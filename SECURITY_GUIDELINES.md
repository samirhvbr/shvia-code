# SHVIA-CODE — Diretrizes de Segurança

> Segurança do projeto **como um todo**: o CLI `anna` (este repo), o endpoint
> `/api/v1/code/chat` (repo `SHVIA-WEB`) e a operação diária. Um CLI agêntico
> **lê arquivos, edita e roda comandos na máquina do dev** — a superfície de
> ataque é real e este documento é vinculante. Irmão do
> `SHVIA-WEB/SECURITY_GUIDELINES.md` (servidor).

---

## 1. Modelo de ameaça (o que pode dar errado)

| # | Ameaça | Vetor |
|---|--------|-------|
| A1 | **Prompt injection via conteúdo lido** | um arquivo/README/log malicioso no repo instrui o modelo a "rodar X / exfiltrar Y" |
| A2 | **Exfiltração de segredos** | o modelo pede `read_file .env` / greps por chaves e o conteúdo segue pro provedor (nuvem) |
| A3 | **Comando destrutivo** | `rm -rf`, `git push --force`, `dd`, migração de banco, etc. sugeridos pelo modelo |
| A4 | **Vazamento da API key do ShvIA** | chave commitada, logada ou lida por outra ferramenta |
| A5 | **Sessões locais expostas** | transcripts contêm trechos de código/segredos lidos |
| A6 | **Abuso do endpoint** | uso sem auth, payload gigante, loop infinito de tools, estouro de quota |
| A7 | **Supply chain** | dependência Rust maliciosa/comprometida no binário que roda em toda máquina de dev |
| A8 | **MITM / host falso** | CLI apontado pra um gateway impostor |

## 2. Regras inegociáveis (cliente `anna`)

1. **Nada escreve ou roda sem o dev ver.** Escrita = diff + confirmação;
   comando fora da allowlist = confirmação; **destrutivo confirma SEMPRE**
   (lista dura: `rm -rf`, `git push -f/--force`, `git reset --hard`,
   `git clean -fd`, `dd`, `mkfs`, `curl|sh`, `chmod -R 777`, drop/truncate).
   **Não existe modo yolo no v1.** (mitiga A1, A3)
2. **Resultado de ferramenta é DADO, não instrução.** O conteúdo lido de
   arquivos entra no prompt marcado como resultado de tool; instruções embutidas
   nele não relaxam gate nenhum — aprovação é do dev, nunca do modelo. (A1)
3. **Denylist de leitura no cliente**: `read_file`/`grep`/`glob` recusam por
   padrão `.env*` (exceto `.env.example`), `*.pem`, `*.key`, `*.p8/p12/pfx`,
   `id_rsa*`, `*credentials*`, `~/.ssh`, `~/.aws`, `.git/config` com URL
   credenciada — mesmo padrão do deny dos `.claude` da casa. Override só com
   confirmação explícita do dev na hora. (A2)
   The list is checked against the requested path **and** the real file after every
   symlink, by `read_file`, `grep`, `edit_file` and `write_file`. Until 0.13.6 the `grep`
   tool did not apply it at all, and a symlink inside the project reached denied files
   (external review, 26/10/2026). In `bash`, an unquoted glob in a content-printing command
   asks, because the shell expands it after the argument check.
4. **API key**: mora em `~/.config/shvia-code/config` com permissão **0600**
   (ou keychain do SO no futuro), **nunca** em arquivo do projeto, nunca em
   argumento de linha de comando (aparece no `ps`), **nunca** impressa em log
   ou transcript. Env `SHVIA_API_KEY` aceita para CI. Revogável a qualquer
   momento no painel do ShvIA. (A4)
5. **Sessões locais** em `~/.shvia-code/sessions/` com **0600/0700**, fora de
   qualquer sync de nuvem; `anna sessions clear` apaga tudo. Transcripts nunca
   sobem ao servidor no v1 (D7). (A5)
6. **Transporte**: HTTPS obrigatório; host default `ai.shvia.org`; trocar de
   host exige edição manual da config (nunca por flag do modelo/tool). Erro de
   certificado = aborta, sem opção interativa de "ignorar". (A8)
7. **Limites no loop**: teto de voltas por turno (default 100 — `max_voltas` no
   config.toml / env `SHVIA_MAX_VOLTAS`, nunca ilimitado) e teto de bytes por
   resultado de tool. Estouro do teto de voltas: o terminal interativo pergunta
   ao dev se libera mais um lote; o host NDJSON persistente encerra o turno com
   a sessão viva no processo ("continua" retoma); `-p`/`--json -p` terminam sem
   persistir nada — o aviso manda rodar de novo com teto maior. A pergunta NUNCA viaja pelo
   `gate_request`: o modo Auto de um host a aprovaria sozinho — loop sem humano,
   exatamente o que este teto impede. O default subiu de 25 em 19/08/2026:
   tarefa legítima (testes+correção em projeto Godot) morria no meio com 26
   ferramentas; as proteções reais de custo seguem sendo o gate por ferramenta,
   o teto do gateway (§3.3) e a telemetria por volta. (A6)

## 3. Regras inegociáveis (servidor `/api/v1/code/chat`, repo SHVIA-WEB)

1. **Auth obrigatória** (API key/Sanctum — guard existente) + **rate limit**
   próprio; quotas de tokens já existentes valem. (A6)
2. **O servidor NÃO executa ferramenta nenhuma** — é stateless de tools: recebe
   defs, devolve `tool_call`. Execução é 100% no cliente. Superfície server-side
   nova ≈ zero. (A3, A6)
3. **Validação de payload**: nº máx. de messages/tools, tamanho máx. de
   content/arguments, profundidade de JSON — recusa educada acima do teto. (A6)
4. **LGPD herdada**: `maskOutboundPii()` na saída pra nuvem e `data_locality`
   continuam valendo — automáticos, sem opção de desligar pelo CLI. (A2)
5. **Auditoria integral**: toda chamada vira `inference_requests` (tokens,
   custo, duração, request_id) — **métricas, não conteúdo**: nenhum payload de
   código em log de servidor. (A5)
6. Erros nunca ecoam a API key nem headers de auth.

## 4. Supply chain & build (A7)

- `Cargo.lock` **versionado**; dependências mínimas e conhecidas (tokio,
  reqwest/rustls, serde, clap/ratatui ou similares) — nada exótico sem revisão.
- `cargo audit` no `build-local.sh` (padrão preflight 0.5.8: falha cedo e
  acionável) + `cargo clippy -D warnings` antes de release.
- Binários distribuídos com checksum (SHA-256 publicado junto). TLS via
  `rustls` (sem depender de OpenSSL da máquina).

## 5. LGPD / propriedade intelectual

- **Modelos livres (ADR-001/D6)** é decisão consciente: o dev escolhe o perfil,
  e código enviado a provedor de nuvem sai da Blue3 — quem precisa de sigilo
  total usa perfil **on-prem** (a UI do CLI sempre mostra o modelo/infra ativos
  no cabeçalho da sessão, pra escolha ser visível, nunca silenciosa).
- Masking de PII server-side é automático e inegociável (§3.4).

## 6. Checklist de release (cada versão)

- [ ] `cargo audit` limpo · `cargo clippy -D warnings` limpo · testes verdes
- [ ] Denylist de leitura e lista de destrutivos cobertas por teste
- [ ] Nenhum segredo em log/transcript (teste com chave fake)
- [ ] `php -l` + smoke do endpoint no SHVIA-WEB quando ele mudar
- [ ] Checksums publicados com os binários

## 7. Relato de vulnerabilidade

Achou algo? **Não abra issue pública.** Avise o Samir (interno Blue3) com
passos de reprodução; correção sai como hotfix com bump Z e nota no ADR se
mudar postura.
