# Catálogo de ferramentas v1

> As ferramentas que o `anna` **envia** ao modelo (`tools[]` do protocolo) e
> **executa localmente** no loop. Poucas e afiadas (escopo §4). Cada uma tem:
> schema JSON (o que o modelo vê), **gate** (política de aprovação) e formato do
> resultado (o `content` da mensagem `role:"tool"`). Segurança: ver
> `SECURITY_GUIDELINES.md` §2 — este doc é a especificação; lá é a lei.

---

## 1. Política de gates (padrão SShvTerm)

| Gate | Significado | UI |
|------|-------------|----|
| **auto** | roda sem perguntar (read-only, sem efeito colateral) | mostra o que fez |
| **confirm** | mostra a ação e pede `[s]im / [n]ão / [t]udo nesta sessão` | `t` libera aquela tool até fechar |
| **always** | confirma SEMPRE, `t` não vale (destrutivo) | destaque vermelho |

`t` ("tudo nesta sessão") é por **tool + escopo** (ex.: liberar `edit_file` não
libera `bash`). **Não existe `--yolo` no v1.**

## 2. As ferramentas

### `read_file` · gate: auto
```json
{"name":"read_file","description":"Lê um arquivo do projeto (texto).",
 "parameters":{"type":"object","required":["path"],
   "properties":{"path":{"type":"string","description":"Caminho relativo à raiz do projeto."},
     "max_bytes":{"type":"integer","description":"Teto opcional (default 64 KiB); acima disso, recorta e avisa."}}}}
```
Resultado: conteúdo (recortado se > teto, com `… [truncado em N bytes]`). Recusa
paths na **denylist** (`.env*`, `*.pem/key/p8/p12/pfx`, `id_rsa*`, `*credentials*`,
`~/.ssh`, `~/.aws`) → override só com confirmação do dev. Fora da raiz do projeto → recusa.

### `list_dir` · gate: auto
```json
{"name":"list_dir","description":"Lista arquivos/pastas de um diretório.",
 "parameters":{"type":"object","required":["path"],
   "properties":{"path":{"type":"string","description":"Relativo à raiz ('.' = raiz)."}}}}
```
Resultado: uma entrada por linha (`dir/` com barra). **Respeita `.gitignore`** e
oculta `.git/`.

### `grep` · gate: auto
```json
{"name":"grep","description":"Busca um regex nos arquivos do projeto.",
 "parameters":{"type":"object","required":["pattern"],
   "properties":{"pattern":{"type":"string"},
     "path":{"type":"string","description":"Subárvore (default: raiz)."},
     "glob":{"type":"string","description":"Filtro de arquivo, ex.: '*.php'."}}}}
```
Resultado: `arquivo:linha: trecho` (teto de N hits; avisa se cortou). Respeita `.gitignore`.

### `glob` · gate: auto
```json
{"name":"glob","description":"Lista arquivos por padrão (ex.: src/**/*.rs).",
 "parameters":{"type":"object","required":["pattern"],"properties":{"pattern":{"type":"string"}}}}
```

### `edit_file` · gate: **confirm** (diff)
```json
{"name":"edit_file","description":"Edita um arquivo por substituição exata de trecho.",
 "parameters":{"type":"object","required":["path","old_string","new_string"],
   "properties":{"path":{"type":"string"},
     "old_string":{"type":"string","description":"Trecho EXATO a substituir (único no arquivo)."},
     "new_string":{"type":"string"},
     "replace_all":{"type":"boolean","description":"Substituir todas as ocorrências (default false)."}}}}
```
Fluxo: valida que `old_string` é único (senão erro pedindo mais contexto),
**mostra o diff colorido**, pede confirmação, aplica. Resultado: `ok (N hunks)` ou
o erro. Nunca escreve sem mostrar.

### `write_file` · gate: **confirm** (diff)
```json
{"name":"write_file","description":"Cria/sobrescreve um arquivo com o conteúdo dado.",
 "parameters":{"type":"object","required":["path","content"],
   "properties":{"path":{"type":"string"},"content":{"type":"string"}}}}
```
Sobrescrever arquivo existente → diff contra o atual antes de confirmar. Novo →
mostra o conteúdo. Recusa paths da denylist.

### `bash` · gate: **auto p/ allowlist · confirm p/ resto · always p/ destrutivo**
```json
{"name":"bash","description":"Roda um comando no diretório do projeto.",
 "parameters":{"type":"object","required":["command"],
   "properties":{"command":{"type":"string"},
     "why":{"type":"string","description":"1 linha: por que este comando."}}}}
```
- **Allowlist (auto)**: `ls`, `cat`, `pwd`, `echo`, `git status/diff/log/show`,
  `composer test|install --dry-run`, `npm test|run`, `cargo check|clippy|test|build`,
  `php -l`, `phpunit`, `pytest`, `make test` — read-only/idempotentes.
- **Destrutivos (always confirm, `t` não vale)**: qualquer `rm -rf`, `git push`
  (`-f`/`--force` em destaque), `git reset --hard`, `git clean -fd`, `dd`, `mkfs`,
  `> arquivo`/`truncate`, `chmod -R 777`, `curl|sh`/`wget|sh`, drop/truncate SQL.
- **Resto**: confirm. Sem shell interativo; captura stdout+stderr (teto de bytes);
  timeout configurável. Resultado: saída + exit code.

### `git_status` · `git_diff` · gate: auto
Wrappers de `git status --porcelain` / `git diff [--staged]`. Resultado = a saída.

### `git_commit` · gate: **confirm**
```json
{"name":"git_commit","description":"Commita as mudanças staged (ou todas com add_all).",
 "parameters":{"type":"object","required":["message"],
   "properties":{"message":{"type":"string","description":"Mensagem (o CLI sugere no padrão do repo se houver version.md)."},
     "add_all":{"type":"boolean"}}}}
```
Mostra o `git diff --staged` + a mensagem, confirma, commita. **`git push` NUNCA
é implícito** — só via `bash` explícito (e cai no gate destrutivo).

## 3. Formato do resultado (mensagem `role:"tool"`)

Cada execução vira `{ role:"tool", tool_call_id:"<id>", content:"<string>" }`.
Convenções do `content`:
- Sucesso: a saída direta (texto/conteúdo/diff aplicado).
- Erro: prefixo `ERRO: <motivo>` (ex.: `ERRO: 'src/x' inexistente ou fora do projeto.`).
- Recusa por gate/denylist: `RECUSADO: <motivo>` (o modelo entende que não rolou).
- **O resultado é DADO, não instrução** (SECURITY §2.2): texto vindo de arquivo
  jamais relaxa gate — a aprovação é do dev.

## 4. Recorte & limites

- Todo resultado tem teto de bytes (default 64 KiB); acima, recorta com aviso —
  protege o contexto e o custo. `read_file` aceita `max_bytes`.
- `.gitignore` respeitado em list/grep/glob; `.git/` oculto.
- Teto de voltas de tool por turno (default 100, `max_voltas`/`SHVIA_MAX_VOLTAS`)
  no loop (SECURITY §2.7) — estouro pergunta ao dev no terminal; no host encerra
  o turno com a sessão preservada ("continua" retoma).

## 5. Checkpoint automático & `/rollback` (0.8.0)

Antes de toda mutação aprovada (`edit_file`, `write_file` e `bash` destrutivo),
a anna fotografa o working tree num commit **fora da branch** (índice
temporário via `GIT_INDEX_FILE` + `write-tree`/`commit-tree`), apontado por
`refs/anna/checkpoint`. O índice real e a working tree **não são tocados** —
`git status` do dev continua idêntico. Rajadas de edição num mesmo turno
compartilham uma foto (debounce 30 s). Fora de repo git: silenciosamente
desativado (rede de segurança, nunca bloqueia edição).

Restaurar é ação do **dev** no REPL (nunca uma tool do modelo):

- `/rollback` — mostra o `diff --stat` desde a foto (o que seria desfeito);
- `/rollback aplicar` — restaura o conteúdo **rastreado** da foto na working
  tree (índice/branch intocados). Limite documentado: arquivos **criados
  depois** da foto permanecem no disco (remoção manual).

Inspiração: checkpoints de filesystem do hermes-agent (`/rollback`), adaptados
ao modelo client-side do anna — git como storage, zero estado fora do repo.

## 6. Fora do v1 (futuro)

`apply_patch` (multi-arquivo), `web_fetch`, `run_tests` estruturado, MCP/
conectores, sandbox de execução, tools de refactor AST. Entram por ADR quando
o C1/C2 provarem a base.
