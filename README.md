# Ship of Theseus 🛳️
### LLM-Integrated Research Station & Hardware Monitor

## Overview
This project is a custom-built GUI workstation designed to assist in my studies as an **Electrical Engineering student at Texas State University** (Concentration: Micro and Nano Device Systems). 

The goal of the **Ship of Theseus** is to create a seamless interface between local LLMs (via Ollama) and my technical research library.

## Key Features
* **VRAM Monitoring:** Real-time tracking of GPU resources using `nvidia-smi` to manage dual-GPU workflows.
* **RAG (Retrieval-Augmented Generation):** A custom scanner that parses local PDFs (circuit datasheets, signal processing notes) to provide context-aware AI responses.
* **State-Machine Architecture:** Built in **Rust** using `eframe/egui`, utilizing an async messaging system to keep the UI responsive during heavy "Thinking" or "Scanning" states.
* **Active Learning:** This repository documents my journey self-teaching Rust and Fedora Linux.

## Technical Stack
* **Language:** Rust
* **GUI:** egui
* **Backend:** Ollama (Models: `gemma3:27b`, `gpt-oss:20b`)
* **OS:** Fedora Linux

## Project Roadmap
- [ ] Implement advanced PDF chunking for better RAG accuracy.
- [ ] Add visualization for Signal Processing data.
- [ ] Integrate hardware diagnostics for GPU repair projects (AMD Radeon RX 5700).

---
**Author:** Raul Montoya Cardenas  
*Aspiring R&D Engineer in BioTech and Automation Robotics*
