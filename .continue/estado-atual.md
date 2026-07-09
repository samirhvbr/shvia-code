# Estado atual — SHVIA-CODE

- **08/07/2026** — Nasce o repo com o **escopo aprovado** (pré-código).
  Decisões travadas: comando `anna` · Rust · modelos livres (ADR-001).
- **Próximo passo: C0** — spike do gateway:
  1. No `SHVIA-WEB`: expor `POST /api/v1/code/chat` (SSE) com `tools[]`,
     `tool_choice` e `messages[]` aceitando `role=tool` (o
     `ChatService::callOllama` já tem os parâmetros — é ligar a borda HTTP).
  2. Script de prova: pergunta → `tool_call` → resultado → resposta final,
     com um modelo on-prem E um de nuvem (mede a qualidade de tool-calling).
  3. DoD: transcript do loop fechado, colado aqui.
