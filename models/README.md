# Moneta Local Models Directory

Place your offline GGUF models in these directories:

- `models/llm/`
  Recommended LLM GGUF models (quantized, fast on standard CPUs):
  - Qwen2.5-0.5B-Instruct-Q4_K_M.gguf (compact ~400MB, extremely fast)
  - Qwen2.5-1.5B-Instruct-Q4_K_M.gguf (~1GB)
  - Llama-3.2-1B-Instruct-Q4_K_M.gguf (~800MB)

- `models/embeddings/`
  Recommended Embedding GGUF models:
  - bge-small-en-v1.5-q4_k_m.gguf (~45MB)
  - all-MiniLM-L6-v2-q4_k_m.gguf (~30MB)
  - nomic-embed-text-v1.5.Q4_K_M.gguf

Moneta will automatically detect any `.gguf` file placed in these folders, or you can configure exact paths via environment variables or settings:
- `MONETA_LLM_MODEL`: Path to LLM `.gguf` file
- `MONETA_EMBEDDING_MODEL`: Path to embedding `.gguf` file
