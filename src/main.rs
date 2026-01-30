#[cfg(feature = "gui")]
mod gui {
    use eframe::egui;
    use std::fs;
    use std::process::Command;
    use serde::{Deserialize, Serialize};
    use std::sync::mpsc;
    use std::thread;
    use arboard::Clipboard;

    // Ollama Imports
    use ollama_rs::Ollama;
    use ollama_rs::generation::chat::{ChatMessage, MessageRole};
    use ollama_rs::generation::chat::request::ChatMessageRequest;
    use ollama_rs::generation::images::Image;

    // --- 1. DATA STRUCTURES ---

    const SESSIONS_DIR: &str = "sessions";
    // Your custom system profile
    const USER_PROFILE: &str = "You are an Electrical Engineering student at Texas State University named Raul. You have a strong background in circuits, signal processing, and embedded systems. Concentration on Micro and Nano Device Systems. Always provide detailed explanations and practical examples."; 

    #[derive(Serialize, Deserialize, Clone, Debug)]
    struct Message {
        role: String,
        has_image: bool,
        content: String, 
    }

    // [NEW] The State Machine for the GUI
    #[derive(PartialEq, Debug)]
    enum AppState {
        Idle,        // Ready for input
        Scanning,    // Currently searching PDFs (RAG)
        Generating,  // Currently waiting for Ollama (LLM)
    }

    struct ShipApp {
        // UI State
        input_text: String,
        current_file: String,
        messages: Vec<Message>,
        models: Vec<String>,
        selected_model: String,
        vram_usage: (u64, u64),
        
        // Research & Agent State
        state: AppState,           // [CHANGED] Replaces simple booleans
        research_results: String,  // Buffer for search results
        research_dir: String,      // Path to your research docs
        is_reasoning_mode: bool,   // Toggle for "Deep Research" logic
        
        // Vision & Context Buffers
        current_image_base64: Option<String>,
        current_image_path: Option<String>,

        // Async Communication
        tx: std::sync::mpsc::Sender<String>, 
        rx: std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<String>>>, 
    }

    impl ShipApp {
        fn new(_cc: &eframe::CreationContext<'_>) -> Self {
            // Create sessions directory
            let _ = fs::create_dir_all(SESSIONS_DIR);

            // Async Channel (using std sync mpsc)
            let (tx, rx) = std::sync::mpsc::channel::<String>();

            Self {
                input_text: String::new(),
                current_file: "session_latest.json".to_string(),
                messages: Vec::new(),
                // My Models
                models: vec!["gemma3:27b".to_string(), "gpt-oss:20b".to_string()], 
                selected_model: "gemma3:27b".to_string(),
                vram_usage: (0, 0),
                
                // Initialize State Machine
                state: AppState::Idle,
                research_results: String::new(),
                research_dir: String::from("/home/raulmc/Documents"), // Your Default Path
                is_reasoning_mode: false,
                // [FIX] Error line removed here
                current_image_base64: None,
                current_image_path: None,
                
                tx: tx,
                rx: std::sync::Arc::new(std::sync::Mutex::new(rx)),
            }
        }

        // Helper to get VRAM from nvidia-smi
        fn get_vram_usage() -> (u64, u64) {
            let output = Command::new("nvidia-smi")
                .args(&["--query-gpu=memory.used,memory.total", "--format=csv,noheader,nounits"])
                .output();

            if let Ok(o) = output {
                let s = String::from_utf8_lossy(&o.stdout);
                if let Some(line) = s.lines().next() {
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() == 2 {
                        let used = parts[0].trim().parse::<u64>().unwrap_or(0);
                        let total = parts[1].trim().parse::<u64>().unwrap_or(0);
                        return (used, total);
                    }
                }
            }
            (0, 0)
        }

        // [FIXED] The Async RAG Scanner (Non-blocking)
        fn scan_research(&mut self, keyword: String) {
            let dir = self.research_dir.clone(); 
            let tx = self.tx.clone();
            
            // 1. Update State to block double-clicks
            self.state = AppState::Scanning;

            // 2. Spawn thread (blocking)
            std::thread::spawn(move || {
                let mut found_data = String::new();
                let pattern = format!("{}/**/*.pdf", dir);
                
                // Send status update
                let _ = tx.send(format!("__STATUS__: Scanning for signal '{}'...", keyword));

                if let Ok(paths) = glob::glob(&pattern) {
                    for entry in paths.flatten() {
                        if let Ok(content) = pdf_extract::extract_text(&entry) {
                            if content.to_lowercase().contains(&keyword.to_lowercase()) {
                                let filename = entry.file_name().unwrap_or_default().to_string_lossy();
                                
                                // Get context window
                                let snippet = ShipApp::get_relevant_snippet(&content, &keyword);
                                found_data.push_str(&format!("\n[SOURCE: {}]\n{}\n", filename, snippet));
                            }
                        }
                    }
                }
                
                if found_data.is_empty() {
                    // Signal completion with no data
                    let _ = tx.send("__RESEARCH_EMPTY__".to_string());
                } else {
                    // Signal completion WITH data
                    let _ = tx.send(format!("__RESEARCH_DATA__:{}", found_data));
                }
            });
        }

        // Helper to grab text around the keyword
        fn get_relevant_snippet(content: &str, keyword: &str) -> String {
            let lower_content = content.to_lowercase();
            let lower_keyword = keyword.to_lowercase();

            if let Some(index) = lower_content.find(&lower_keyword) {
                let start = index.saturating_sub(200);
                let end = (index + 500).min(content.len()); // Increased context window
                content[start..end].to_string()
            } else {
                String::new()
            }
        }

        // [NEW] Trigger Ollama (Called after research OR directly)
        fn trigger_ollama_generation(&mut self, prompt: String) {
            self.state = AppState::Generating;
            let tx_clone = self.tx.clone();
            let model = self.selected_model.clone();
            let img_data = self.current_image_base64.clone();
            let research_context = self.research_results.clone();
            
            // Clear buffer now that we are using it
            self.research_results.clear();

            // Spawn Ollama Task
            std::thread::spawn(move || {
                 // Create a tokio runtime to run async Ollama calls
                 let rt = tokio::runtime::Runtime::new().unwrap();
                 let ollama = Ollama::default();
                 
                 // 1. Build History
                 let mut api_history = Vec::new();
                 api_history.push(ChatMessage::new(MessageRole::System, USER_PROFILE.to_string()));
                 
                 // 2. Construct Final Prompt
                 let final_content = if !research_context.is_empty() {
                     format!("### RESEARCH DATA:\n{}\n\n### USER QUERY:\n{}", research_context, prompt)
                 } else {
                     prompt
                 };

                 // 3. Create Message
                 let mut user_msg = ChatMessage::new(MessageRole::User, final_content);
                 
                 // 4. Attach Image if present
                 if let Some(b64) = img_data {
                     user_msg.images = Some(vec![Image::from_base64(&b64)]);
                 }
                 
                 api_history.push(user_msg);
                 
                 let request = ChatMessageRequest::new(model, api_history);
                 
                 // 5. Stream Response (blocking via runtime)
                 match rt.block_on(ollama.send_chat_messages(request)) {
                     Ok(response) => {
                         if let Some(message) = response.message {
                             let _ = tx_clone.send(message.content);
                         }
                     }
                     Err(_) => {
                         let _ = tx_clone.send("Error: Failed to connect to Ollama.".to_string());
                     }
                 }
                 let _ = tx_clone.send("__DONE__".to_string());
            });
            
            // Reset image buffer immediately
            self.current_image_base64 = None;
        }
    }

    impl eframe::App for ShipApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // 1. Hardware Monitor
            self.vram_usage = Self::get_vram_usage();

            // 2. Request a repaint every 1 second (1000ms)
            ctx.request_repaint_after(std::time::duration::from_millis(1000));

            // 3. MESSAGE HANDLER (The "Brain" Loop)
            let rx_guard = self.rx.lock().unwrap();
            while let Ok(msg) = rx_guard.try_recv() {
                if msg == "__DONE__" {
                    self.state = AppState::Idle; 
                } 
                else if msg.starts_with("__STATUS__") {
                     // You could log this to a status bar
                     println!("{}", msg);
                }
                else if msg.starts_with("__RESEARCH_DATA__") {
                    // RAG Success: Store data and trigger LLM
                    let data = msg.trim_start_matches("__RESEARCH_DATA__");
                    self.research_results = data.to_string();
                    
                    // Retrieve the user's last message to use as the prompt
                    if let Some(last_msg) = self.messages.last() {
                        if last_msg.role == "user" {
                            let prompt = last_msg.content.clone();
                            self.trigger_ollama_generation(prompt);
                        }
                    }
                }
                else if msg == "__RESEARCH_EMPTY__" {
                    // RAG Fail: Just trigger LLM without data
                    if let Some(last_msg) = self.messages.last() {
                        if last_msg.role == "user" {
                            let prompt = last_msg.content.clone();
                            self.trigger_ollama_generation(prompt);
                        }
                    }
                }
                else {
                    // Streamed Token from Ollama
                    if let Some(last_msg) = self.messages.last_mut() {
                        if last_msg.role == "assistant" {
                            last_msg.content.push_str(&msg);
                        } else {
                            self.messages.push(Message {
                                role: "assistant".to_string(),
                                content: msg,
                                has_image: false,
                            });
                        }
                    }
                }
            }

            // 4 . GUI LAYOUT
            egui::SidePanel::left("sidebar").show(ctx, |ui| {
                ui.heading("Ship of Theseus 🛳️");
                ui.separator();
                ui.label(format!("VRAM: {} / {} MB", self.vram_usage.0, self.vram_usage.1));
                ui.separator();
                
                // Model Selector
                ui.label("Active Neural Net:");
                egui::ComboBox::from_id_source("model_selector")
                    .selected_text(&self.selected_model)
                    .show_ui(ui, |ui| {
                        for model in &self.models {
                            ui.selectable_value(&mut self.selected_model, model.clone(), model);
                        }
                    });

                ui.separator();
                ui.label("Research Station 🔬");
                ui.checkbox(&mut self.is_reasoning_mode, "Reasoning Mode (RAG)");
                ui.text_edit_singleline(&mut self.research_dir);
                ui.small("Point this to your PDFs folder");
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                // Chat History
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for msg in &self.messages {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&msg.role).strong());
                            ui.label(&msg.content);
                        });
                        ui.separator();
                    }
                });

                ui.separator();

                // Input Area
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.input_text);
                    
                    // Dynamic Button Label
                    let btn_text = match self.state {
                        AppState::Idle => "Send",
                        AppState::Scanning => "Scanning...",
                        AppState::Generating => "Thinking...",
                    };

                    // SEND LOGIC
                    if ui.button(btn_text).clicked() && self.state == AppState::Idle {
                        let user_text = self.input_text.clone();
                        
                        // Add User Message to UI immediately
                        self.messages.push(Message {
                            role: "user".to_string(),
                            content: user_text.clone(),
                            has_image: self.current_image_base64.is_some(),
                        });
                        self.input_text.clear();

                        // DECISION TREE: Research vs. Chat
                        if self.is_reasoning_mode {
                            // Path A: Scan Docs -> Then Chat
                            self.scan_research(user_text); 
                        } else {
                            // Path B: Chat Directly
                            self.trigger_ollama_generation(user_text);
                        }
                    }
                });
            });
        }
    }

    pub fn run() -> Result<(), eframe::Error> {
        let options = eframe::NativeOptions::default();
        eframe::run_native(
            "Ship of Theseus",
            options,
            Box::new(|cc| Box::new(ShipApp::new(cc))),
        )
    }
}

#[cfg(feature = "gui")]
fn main() -> Result<(), eframe::Error> {
    gui::run()
}

#[cfg(not(feature = "gui"))]
fn main() {
    println!("GUI feature not enabled; build with `--features gui` and add the `eframe` dependency to Cargo.toml to enable the GUI.");
}