import streamlit as st
import ollama
from PIL import Image
import io
import pypdf
import json
import os
import subprocess
import glob
from datetime import datetime
import pytesseract 
from pdf2image import convert_from_bytes

# --- 1. SYSTEM ARCHITECTURE ---
SESSIONS_DIR = "sessions"  # Folder to store all chat files
USER_PROFILE = "User: Raul (EE Student, TXST). System: Ship of Theseus (RTX 5080)."

if not os.path.exists(SESSIONS_DIR):
    os.makedirs(SESSIONS_DIR)

st.set_page_config(
    page_title="Ship of Theseus",
    page_icon="⚡",
    layout="wide",
    initial_sidebar_state="expanded"
)

# Custom CSS for the "Chat History" sidebar look
st.markdown("""
<style>
    .stDeployButton {display:none;}
    footer {visibility: hidden;}
    .stButton button {
        width: 100%;
        text-align: left;
    }
</style>
""", unsafe_allow_html=True)

# --- 2. DATABASE CONTROLLERS ---

def get_vram_usage():
    try:
        result = subprocess.run(
            ['nvidia-smi', '--query-gpu=memory.used,memory.total', '--format=csv,nounits,noheader'],
            stdout=subprocess.PIPE, text=True
        )
        used, total = map(int, result.stdout.strip().split(','))
        return used, total
    except:
        return 0, 16384

def create_new_session():
    # Generate a unique filename based on time
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    filename = f"chat_{timestamp}.json"
    
    # Reset state
    st.session_state["messages"] = []
    st.session_state["current_file"] = filename
    st.rerun()

def load_session(filename):
    filepath = os.path.join(SESSIONS_DIR, filename)
    with open(filepath, "r") as f:
        st.session_state["messages"] = json.load(f)
    st.session_state["current_file"] = filename
    st.rerun() # Force reload UI

def save_current_session():
    # If no file is selected, create one
    if "current_file" not in st.session_state:
        create_new_session()
        
    filepath = os.path.join(SESSIONS_DIR, st.session_state["current_file"])
    with open(filepath, "w") as f:
        json.dump(st.session_state["messages"], f, indent=2)

# --- 3. BOOT SEQUENCE ---
# If we just opened the app and have no file selected, create a new one
if "current_file" not in st.session_state:
    # Check if there are existing sessions to load the latest one?
    # For now, let's start fresh or load latest.
    files = sorted(glob.glob(os.path.join(SESSIONS_DIR, "*.json")), reverse=True)
    if files:
        latest_file = os.path.basename(files[0])
        st.session_state["current_file"] = latest_file
        with open(files[0], "r") as f:
            st.session_state["messages"] = json.load(f)
    else:
        # Total fresh start
        create_new_session()

if "messages" not in st.session_state:
    st.session_state["messages"] = []

# --- 4. SIDEBAR (HISTORY & CONFIG) ---
with st.sidebar:
    st.title("⚡ Ship of Theseus")
    
    # --- NEW CHAT BUTTON ---
    if st.button("➕ New Chat", type="primary"):
        create_new_session()
    
    st.divider()
    st.subheader("Recent Sessions")
    
    # --- HISTORY LIST ---
    # List all .json files in the sessions folder
    files = sorted(glob.glob(os.path.join(SESSIONS_DIR, "*.json")), reverse=True)
    
    for filepath in files:
        filename = os.path.basename(filepath)
        
        # Try to find a "Name" for the chat (First user message)
        chat_label = filename # Default to date
        try:
            with open(filepath, 'r') as f:
                data = json.load(f)
                # Find first user message for the title
                for msg in data:
                    if msg['role'] == 'user':
                        # Cut it to 25 chars so it fits
                        chat_label = msg['content'][:25] + "..."
                        break
        except:
            pass

        # Highlight the active chat
        if filename == st.session_state.get("current_file"):
            st.info(f"📂 {chat_label}")
        else:
            if st.button(f"📄 {chat_label}", key=filename):
                load_session(filename)

    st.divider()
    
    # --- SYSTEM STATS ---
    vram_used, vram_total = get_vram_usage()
    st.caption(f"VRAM: {vram_used}MB / {vram_total}MB")
    st.progress(vram_used / vram_total)
    
    model_choice = st.selectbox("Engine", ["gemma2:27b", "deepseek-r1:14b", "llava"])

   # --- FILE UPLOAD ---
    uploaded_file = st.file_uploader("Context (PDF/IMG)", type=['jpg', 'png', 'pdf'])
    
    file_context = None
    has_image = False
    
    if uploaded_file.type == "application/pdf":
            text = ""
            try:
                # 1. Try Fast Text Extraction (Digital PDFs)
                pdf_reader = pypdf.PdfReader(uploaded_file)
                for page in pdf_reader.pages:
                    extracted = page.extract_text()
                    if extracted: text += extracted

                # 2. Fallback to OCR (Scanned PDFs) if text is empty
                if not text.strip():
                    st.warning("⚠️ Scan detected. Engaging OCR (Optical Character Recognition)...")
                    with st.spinner("Compiling pixels to text (This takes CPU power)..."):
                        # Reset file pointer to start
                        uploaded_file.seek(0)
                        # Convert PDF pages to Images
                        images = convert_from_bytes(uploaded_file.read())
                        # Read text from each image
                        for img in images:
                            text += pytesseract.image_to_string(img)
            
                file_context = text
                st.success(f"📄 Data Extracted: {len(text)} chars")
                
            except Exception as e:
                st.error(f"Read Error: {e}")

        else:
            image = Image.open(uploaded_file)
            st.image(image, width=200)
            img_byte_arr = io.BytesIO()
            image.save(img_byte_arr, format=image.format)
            file_context = img_byte_arr.getvalue()
            has_image = True

# --- 5. MAIN CHAT WINDOW ---

# Header showing which file we are editing
st.caption(f"Session ID: {st.session_state['current_file']}")

# Display Messages
for msg in st.session_state.messages:
    with st.chat_message(msg["role"]):
        st.markdown(msg["content"])

# Chat Input
if prompt := st.chat_input("Input..."):
    # 1. User Message
    st.session_state.messages.append({"role": "user", "content": prompt})
    with st.chat_message("user"):
        st.markdown(prompt)
    
    # Auto-save immediately so the title updates in sidebar
    save_current_session()

    # 2. AI Response
    with st.chat_message("assistant"):
        response_placeholder = st.empty()
        full_response = ""
        
        # Build Context
        api_messages = [{'role': 'system', 'content': f"System: {USER_PROFILE}"}]
        
        if file_context and not has_image:
            api_messages.append({'role': 'system', 'content': f"Document: {file_context}"})
            
        # Context window (last 10 messages)
        for msg in st.session_state.messages[-10:]:
            api_messages.append({'role': msg['role'], 'content': msg['content']})

        try:
            if has_image and model_choice == "llava":
                response = ollama.chat(model='llava', messages=[{'role': 'user', 'content': prompt, 'images': [file_context]}])
                full_response = response['message']['content']
                response_placeholder.markdown(full_response)
            else:
                stream = ollama.chat(model=model_choice, messages=api_messages, stream=True)
                for chunk in stream:
                    if chunk['message']['content']:
                        full_response += chunk['message']['content']
                        response_placeholder.markdown(full_response + "▌")
                response_placeholder.markdown(full_response)

            # 3. Save to Disk
            st.session_state.messages.append({"role": "assistant", "content": full_response})
            save_current_session()
            
            # Rerun occasionally to update sidebar title if it was the first message
            if len(st.session_state.messages) <= 2:
                st.rerun()

        except Exception as e:
            st.error(f"Error: {e}")