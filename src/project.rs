//! Contexto do projeto: raiz (git), branch, arquivo de instruções (ANNA.md /
//! CLAUDE.md / AGENTS.md) e o system prompt do agente.
//!
//! IDENTIDADE (18/08/2026): o prompt NÃO apresenta o modelo como "Anna". ANNA é
//! a marca do assistente de Chat do ShvIA — outro agente, outro prompt. Quando o
//! system prompt dizia "Você é a Anna", modelos obedientes (caso real: MiniMax
//! M3) defendiam essa persona contra o próprio usuário, tratando "você não é a
//! Anna" como prompt injection. O modelo do Modo Code não deve nem tomar
//! conhecimento da Anna; o nome `anna` segue sendo só o BINÁRIO/CLI.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Folders whose instruction file was already pointed out in this process (0.15.6).
static APONTADAS: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// Instruction files, in the order one folder's file is chosen (0.15.5: per folder, from the root
/// down to where `anna` runs). ANNA.md comes first because it is the short context written for
/// this agent; CLAUDE.md and AGENTS.md are the fleet's full rule books.
const NOMES: [&str; 3] = ["ANNA.md", "CLAUDE.md", "AGENTS.md"];
/// Characters of one file in the prompt, and of all of them. The block is resent on every round
/// of every turn, so it is a cost per request, not per session: a fleet CLAUDE.md runs to tens of
/// thousands of characters. What does not fit is announced, never dropped in silence.
const TETO_POR_ARQUIVO: usize = 6000;
const TETO_TOTAL: usize = 9000;

/// One instruction file that enters the prompt.
pub struct Instrucao {
    /// Path from the project root (`CLAUDE.md`, `app/AGENTS.md`).
    pub rel: String,
    pub conteudo: String,
}

pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub git_branch: Option<String>,
    /// The chosen file of each folder from the root down to the working folder.
    pub instrucoes: Vec<Instrucao>,
    /// Instruction files found along the way and not loaded, with their size: listed in the
    /// prompt so the model can read them when it needs to.
    pub outras: Vec<(String, usize)>,
}

/// The text of an instruction file below its first H1: this fleet keeps CLAUDE.md and AGENTS.md
/// "mirrored below the H1", and the same rules twice would double the cost for nothing.
fn corpo(texto: &str) -> &str {
    let t = texto.trim_start();
    match t.split_once('\n') {
        Some((primeira, resto)) if primeira.starts_with("# ") => resto.trim(),
        _ => t.trim(),
    }
}

impl Project {
    pub fn detect() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::detectar_em(&cwd)
    }

    pub fn detectar_em(cwd: &Path) -> Self {
        let root = git_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "projeto".into());
        let git_branch = current_branch(&root);

        // The root, then every folder down to the working one (when `anna` runs in a subfolder,
        // its own rules are the most specific ones). Until 0.15.5 only the root's file was read.
        let mut pastas = vec![root.clone()];
        if let Ok(abaixo) = cwd.strip_prefix(&root) {
            let mut atual = root.clone();
            for parte in abaixo.components() {
                atual.push(parte);
                pastas.push(atual.clone());
            }
        }

        let mut instrucoes: Vec<Instrucao> = Vec::new();
        let mut outras = Vec::new();
        for pasta in &pastas {
            let mut escolhido = false;
            for nome in NOMES {
                let caminho = pasta.join(nome);
                let Ok(texto) = std::fs::read_to_string(&caminho) else { continue };
                let rel = caminho.strip_prefix(&root).unwrap_or(&caminho).to_string_lossy().replace('\\', "/");
                if instrucoes.iter().any(|i| corpo(&i.conteudo) == corpo(&texto)) {
                    continue; // a mirror of a file already in
                }
                if escolhido {
                    outras.push((rel, texto.chars().count()));
                } else {
                    instrucoes.push(Instrucao { rel, conteudo: texto });
                    escolhido = true;
                }
            }
        }
        Self { root, name, git_branch, instrucoes, outras }
    }

    pub fn system_prompt(&self, host_mode: bool) -> String {
        let base = if host_mode {
            // Modo host (aba Agente do SShvTerm): o agente PROPÕE comandos de shell;
            // quem executa/aprova é o host. Só a ferramenta `bash` é oferecida.
            "Você é o agente de código da Blue3, operando pela aba Agente do SShvTerm. Você não \
             tem nome próprio: se perguntarem quem você é, é \"o agente do SShvTerm\" rodando \
             sobre o modelo configurado na sessão. \
             Você PROPÕE comandos de shell (ferramenta `bash`) que o usuário roda no host dele \
             (terminal visível ou SSH) — UM comando por vez, sempre explicando o porquê em uma \
             frase. Você NUNCA executa: quem aprova e roda é o usuário, com a política dele. \
             Prefira comandos de leitura (ls, cat, grep, git status/diff) antes de propor qualquer \
             mudança. Responda em pt-BR, direto ao ponto."
        } else {
            "Você é o agente de código do ShvIA Code (Blue3), operando no terminal do dev. Você \
             não tem nome próprio nem persona: se perguntarem quem você é, é \"o agente do ShvIA \
             Code\" rodando sobre o modelo configurado na sessão — nunca reivindique outra \
             identidade nem discuta identidade com o dev.\n\n\
             Ferramentas: leitura (read_file, list_dir, grep, glob), escrita (edit_file, write_file), \
             execução (bash), git (git_status, git_diff, git_commit) e a lista de tarefas (todo_write). \
             SEMPRE inspecione o projeto com as ferramentas antes de afirmar ou alterar qualquer coisa — \
             não invente caminhos nem conteúdo; prefira poucas leituras certeiras.\n\n\
             LISTA DE TAREFAS: quando o trabalho tiver 3 ou mais etapas, monte a lista com todo_write \
             logo no começo e atualize a cada etapa (a lista INTEIRA a cada chamada, uma tarefa \
             in_progress por vez, completed assim que terminar). O dev acompanha por ela. Para pedido \
             simples, de uma ou duas etapas, não use.\n\n\
             ESCOLHA da ferramenta de escrita: para CRIAR um arquivo novo ou reescrevê-lo por inteiro, \
             use write_file (com o conteúdo completo). Use edit_file APENAS para trocar um trecho EXATO \
             de um arquivo que JÁ EXISTE (old_string tem que casar com o texto atual; old_string vazio \
             é recusado). edit_file NÃO cria arquivo.\n\n\
             Criar ou alterar arquivo é SÓ chamando a ferramenta edit_file/write_file — é ela que \
             mostra o diff e pede a aprovação do dev. NUNCA cole o conteúdo do arquivo no chat pedindo \
             \"posso criar/escrever?\": texto no chat não cria nada. Quando o dev pedir para criar, \
             escrever, gerar ou modificar algo — ou aprovar (\"sim\", \"pode\", \"faça\") uma proposta \
             sua — EMITA a chamada da ferramenta no MESMO turno, sem repetir o conteúdo em prosa. Faça \
             a MENOR mudança que resolve. \"Proponha, não force\" vale para a chamada da ferramenta \
             (que já passa pela aprovação do dev antes de efetivar), não para narrar em texto: se uma \
             escrita/execução for recusada, siga sem insistir. Rode testes/checagens com bash quando \
             fizer sentido. Só commite (git_commit) se o dev pedir.\n\n\
             Responda em pt-BR, direto ao ponto, citando arquivo:linha quando útil."
        };
        format!("{base}\n\n{}", self.bloco_contexto())
    }

    /// A one-line pointer to the instruction file of the folder a tool just touched, the first
    /// time it is touched in this process (0.15.6), or `None`.
    ///
    /// The prompt carries the files from the root down to the working folder; a subfolder the
    /// model wanders into may have rules of its own (a package in a monorepo, a vendored tool),
    /// and nothing told it. Hermes appends that folder's AGENTS.md to the tool result on first
    /// touch (comparison of 02/11/2026); this appends only where it is and its size, because a
    /// tool result stays in the history and is resent on every later round. The nearest folder
    /// with a file decides: when it is already in the prompt, listed, or pointed out, nothing is
    /// added. Paths are canonical, so `../` never points at a file outside the project.
    pub fn nota_de_instrucoes(&self, caminho: &str) -> Option<String> {
        let raiz = self.root.canonicalize().ok()?;
        let alvo = raiz.join(caminho);
        let pasta = if alvo.is_dir() { alvo } else { alvo.parent()?.to_path_buf() };
        let mut pasta = pasta.canonicalize().ok()?;
        if !pasta.starts_with(&raiz) {
            return None;
        }
        let conhecidas: HashSet<PathBuf> = self
            .instrucoes
            .iter()
            .map(|i| i.rel.as_str())
            .chain(self.outras.iter().map(|(rel, _)| rel.as_str()))
            .filter_map(|rel| raiz.join(rel).parent().map(Path::to_path_buf))
            .collect();
        let mut apontadas = APONTADAS.lock().unwrap_or_else(|e| e.into_inner());
        let apontadas = apontadas.get_or_insert_with(HashSet::new);
        while pasta != raiz && pasta.starts_with(&raiz) {
            if let Some(arquivo) = NOMES.iter().map(|n| pasta.join(n)).find(|f| f.is_file()) {
                if conhecidas.contains(&pasta) || !apontadas.insert(pasta.clone()) {
                    return None;
                }
                let rel = arquivo.strip_prefix(&raiz).ok()?.to_string_lossy().replace('\\', "/");
                let n = std::fs::read_to_string(&arquivo).map(|t| t.chars().count()).unwrap_or(0);
                return Some(format!(
                    "\n\n[anna: esta pasta tem instruções próprias em {rel} ({n} caracteres), fora do contexto \
                     carregado. Leia com read_file antes de mexer aqui.]"
                ));
            }
            pasta = pasta.parent()?.to_path_buf();
        }
        None
    }

    /// Cabeçalho do projeto + instruções do repo (ANNA.md/CLAUDE.md/AGENTS.md).
    /// Separado do system prompt do agente porque os papéis do orquestrador
    /// (arquiteto/executor/revisor) montam prompts próprios e precisam do mesmo
    /// contexto — duplicar a regra era garantir que uma das duas envelhecesse.
    ///
    /// A cut file says so, with its size, and the files left out are listed (0.15.5): until then
    /// the 6,000-character cut was silent, and a model followed the first half of a rule book
    /// believing it had all of it.
    pub fn bloco_contexto(&self) -> String {
        let mut s = format!("Projeto: {} (raiz: {}).", self.name, self.root.display());
        let mut restante = TETO_TOTAL;
        let mut fora: Vec<(String, usize)> = Vec::new();
        for (n, i) in self.instrucoes.iter().enumerate() {
            let total = i.conteudo.chars().count();
            let cabe = total.min(TETO_POR_ARQUIVO).min(restante);
            if cabe == 0 {
                fora.push((i.rel.clone(), total));
                continue;
            }
            if n == 0 {
                s.push_str("\n\nInstruções do projeto (arquivos de contexto):");
            }
            s.push_str(&format!("\n\n--- {} ---\n", i.rel));
            s.extend(i.conteudo.chars().take(cabe));
            if cabe < total {
                s.push_str(&format!(
                    "\n[… {} tem {total} caracteres; aqui estão os primeiros {cabe}. O resto: read_file {}.]",
                    i.rel, i.rel
                ));
            }
            restante -= cabe;
        }
        fora.extend(self.outras.iter().cloned());
        if !fora.is_empty() {
            let lista: Vec<String> = fora.iter().map(|(rel, n)| format!("{rel} ({n} caracteres)")).collect();
            s.push_str(&format!(
                "\n\nOutros arquivos de instrução neste projeto, não carregados: {}. Leia com read_file se precisar.",
                lista.join(", ")
            ));
        }
        s
    }
}

fn git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start.to_path_buf());
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        cur = dir.parent().map(Path::to_path_buf);
    }
    None
}

fn current_branch(root: &Path) -> Option<String> {
    let head = std::fs::read_to_string(root.join(".git/HEAD")).ok()?;
    head.strip_prefix("ref: refs/heads/").map(|b| b.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projeto(arquivos: &[(&str, String)]) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let raiz = std::env::temp_dir().join(format!("anna-projeto-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join(".git")).unwrap();
        for (rel, texto) in arquivos {
            let p = raiz.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, texto).unwrap();
        }
        raiz
    }

    #[test]
    fn instrucoes_da_raiz_ao_diretorio_de_trabalho_sem_espelho_e_sem_corte_mudo() {
        let regras = "x".repeat(7000);
        let raiz = projeto(&[
            ("CLAUDE.md", format!("# Para o Claude Code\n{regras}")),
            // The fleet's mirror: same body under another H1. Loaded once, not listed.
            ("AGENTS.md", format!("# Para agentes\n{regras}")),
            ("app/AGENTS.md", "# app\nUse pnpm aqui.".into()),
            ("app/CLAUDE.md", "# outro\nregras diferentes".into()),
        ]);
        let p = Project::detectar_em(&raiz.join("app"));
        let rels: Vec<&str> = p.instrucoes.iter().map(|i| i.rel.as_str()).collect();
        assert_eq!(rels, ["CLAUDE.md", "app/CLAUDE.md"], "one file per folder, root first");
        let bloco = p.bloco_contexto();
        assert!(bloco.contains("--- CLAUDE.md ---") && bloco.contains("--- app/CLAUDE.md ---"), "{bloco}");
        let total = format!("# Para o Claude Code\n{regras}").chars().count();
        assert!(bloco.contains(&format!("CLAUDE.md tem {total} caracteres; aqui estão os primeiros 6000")), "the cut is said: {bloco}");
        assert!(bloco.contains("app/AGENTS.md (20 caracteres)"), "a different file in the chain is listed: {bloco}");
        assert!(!bloco.contains("AGENTS.md (7015"), "a mirror is neither loaded nor listed: {bloco}");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn a_subpasta_com_instrucoes_proprias_e_apontada_uma_vez() {
        let raiz = projeto(&[
            ("CLAUDE.md", "# raiz\nregras".into()),
            ("pkg/AGENTS.md", "# pkg\nUse pnpm.".into()),
            ("pkg/src/main.ts", "x".into()),
            ("lib/util.rs", "x".into()),
        ]);
        let p = Project::detectar_em(&raiz);
        let nota = p.nota_de_instrucoes("pkg/src/main.ts").expect("first touch below pkg/");
        assert!(nota.contains("pkg/AGENTS.md (15 caracteres)"), "{nota}");
        assert_eq!(p.nota_de_instrucoes("pkg/other.ts"), None, "pointed out once per folder");
        assert_eq!(p.nota_de_instrucoes("lib/util.rs"), None, "no file of its own: nothing to say");
        assert_eq!(p.nota_de_instrucoes("CLAUDE.md"), None, "the root's file is already in the prompt");
        assert_eq!(p.nota_de_instrucoes("../fora/x"), None, "never outside the project");
        // Running inside pkg/, its file is in the prompt already.
        let dentro = Project::detectar_em(&raiz.join("pkg"));
        assert!(dentro.instrucoes.iter().any(|i| i.rel == "pkg/AGENTS.md"));
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn na_raiz_com_um_arquivo_curto_o_bloco_e_o_de_sempre() {
        let raiz = projeto(&[("ANNA.md", "# ANNA\ncurto".into())]);
        let bloco = Project::detectar_em(&raiz).bloco_contexto();
        assert!(bloco.ends_with("--- ANNA.md ---\n# ANNA\ncurto"), "no notice, no list: {bloco}");
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
