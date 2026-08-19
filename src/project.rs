//! Contexto do projeto: raiz (git), branch, arquivo de instruções (ANNA.md /
//! CLAUDE.md / AGENTS.md) e o system prompt do agente.
//!
//! IDENTIDADE (18/08/2026): o prompt NÃO apresenta o modelo como "Anna". ANNA é
//! a marca do assistente de Chat do ShvIA — outro agente, outro prompt. Quando o
//! system prompt dizia "Você é a Anna", modelos obedientes (caso real: MiniMax
//! M3) defendiam essa persona contra o próprio usuário, tratando "você não é a
//! Anna" como prompt injection. O modelo do Modo Code não deve nem tomar
//! conhecimento da Anna; o nome `anna` segue sendo só o BINÁRIO/CLI.

use std::path::{Path, PathBuf};

pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub git_branch: Option<String>,
    pub context: Option<String>,
}

impl Project {
    pub fn detect() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = git_root(&cwd).unwrap_or(cwd);
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "projeto".into());
        let git_branch = current_branch(&root);
        let context = ["ANNA.md", "CLAUDE.md", "AGENTS.md"]
            .iter()
            .map(|f| root.join(f))
            .find(|p| p.exists())
            .and_then(|p| std::fs::read_to_string(p).ok());
        Self { root, name, git_branch, context }
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
             execução (bash) e git (git_status, git_diff, git_commit). SEMPRE inspecione o projeto com \
             as ferramentas antes de afirmar ou alterar qualquer coisa — não invente caminhos nem \
             conteúdo; prefira poucas leituras certeiras.\n\n\
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

    /// Cabeçalho do projeto + instruções do repo (ANNA.md/CLAUDE.md/AGENTS.md).
    /// Separado do system prompt do agente porque os papéis do orquestrador
    /// (arquiteto/executor/revisor) montam prompts próprios e precisam do mesmo
    /// contexto — duplicar a regra era garantir que uma das duas envelhecesse.
    pub fn bloco_contexto(&self) -> String {
        let mut s = format!("Projeto: {} (raiz: {}).", self.name, self.root.display());
        if let Some(ctx) = &self.context {
            let trimmed: String = ctx.chars().take(6000).collect();
            s.push_str("\n\nInstruções do projeto (arquivo de contexto):\n");
            s.push_str(&trimmed);
        }
        s
    }

    pub fn header(&self, model: &str, server: &str) -> String {
        let git = self.git_branch.clone().unwrap_or_else(|| "sem git".into());
        format!(
            "ANNA · ShvIA Code — projeto {} ({}) · {} @ {}",
            self.name, git, model, server
        )
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
