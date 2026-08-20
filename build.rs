//! A prova do bump: os três portadores de versão concordam, ou o build cai.
//!
//! ## Por que existe, e por que aqui e não num teste
//!
//! Em 20/08/2026 mediu-se o primeiro caso nesta casa: o commit da **0.11.2**
//! (`3da7875`) levou `CHANGELOG.md`, `src/tools.rs` e `version.md` — e **deixou
//! `Cargo.toml` e `Cargo.lock` em `0.11.1`**. O commit anterior, da 0.11.1
//! (`5cd3144`), tinha levado os cinco. Nada acusou: árvore limpa, `cargo build`
//! exit 0, e o binário compilado respondendo **`anna 0.11.1` com o código da
//! 0.11.2**.
//!
//! O custo não é estético. O `stage-anna.mjs` do SHVIA-DESKTOP decide o que
//! empacotar lendo `--version` do binário, e o "Sobre → Motor do Code" (desktop
//! 1.1.25) mostra esse número ao usuário — entregue justamente porque a versão do
//! sidecar não aparecia em lugar nenhum. Um bump pela metade faria ela aparecer
//! **errada**, que é pior que não aparecer: o número existiria e mentiria.
//!
//! **`build.rs` e não `#[test]`** porque o contrato pedido é *"os portadores
//! concordam ou o build cai"*, e `cargo build` não roda testes. Teste protege quem
//! roda a suíte; `build.rs` protege quem só compila — que é exatamente o caminho
//! do `install.sh` e o do empacotamento.
//!
//! ## O que este arquivo guarda, e o que NÃO guarda
//!
//! | Portador | Guardado por |
//! |---|---|
//! | `version.md` × `Cargo.toml` | **este `build.rs`** — provado por reversão: trocar um derruba, nomeando o divergente |
//! | `Cargo.lock` | **`cargo --locked`**, não este arquivo. Medido em 20/08: `cargo build` reescreve o lock em silêncio antes do `build.rs` rodar, então desalinhar só o lock **passa** aqui. Com `--locked` o cargo aborta (exit 101) |
//!
//! Empacotamento e CI devem usar `--locked` — é a única forma de o terceiro
//! portador ser conferido de verdade.
//!
//! **E não é a primeira vez que se confiou em aviso.** O `Cargo.toml` trazia, na
//! linha da versão, o comentário *"O build-local.sh checa"*. Medido em 20/08:
//! **não checava** — todas as referências a `version.md` naquele script são ao
//! `version.md` do próprio desktop. O aviso não só perdeu para o gesto: ele
//! afirmava ser o gesto. Daí este arquivo.
use std::{fs, path::Path};

/// A versão declarada para o pacote `anna` dentro do `Cargo.lock`.
///
/// Procura o bloco `[[package]]` cujo `name = "anna"` e devolve o `version`
/// seguinte. Casar por nome importa: o lock tem centenas de blocos, e o primeiro
/// `version` do arquivo é de outra dependência qualquer.
fn versao_no_lock(lock: &str) -> Option<String> {
    let mut achou_nome = false;
    for linha in lock.lines() {
        let l = linha.trim();
        if l == "name = \"anna\"" {
            achou_nome = true;
        } else if achou_nome {
            if let Some(resto) = l.strip_prefix("version = \"") {
                return resto.strip_suffix('"').map(str::to_string);
            }
            // Bloco novo antes de achar a versão: este não era o nosso.
            if l.starts_with("[[") {
                achou_nome = false;
            }
        }
    }
    None
}

fn main() {
    let raiz = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let raiz = Path::new(&raiz);

    // Sem estes, o cargo cacheia o build.rs e a prova para de rodar depois da
    // primeira vez — uma prova que não roda é pior que nenhuma, porque parece
    // estar de guarda.
    println!("cargo:rerun-if-changed=version.md");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=build.rs");

    // Fonte da verdade declarada: `version.md` (uma linha, só o número).
    let md_path = raiz.join("version.md");
    let md = fs::read_to_string(&md_path)
        .unwrap_or_else(|e| panic!("prova do bump: não consegui ler version.md ({e})"));
    let md = md.trim().to_string();

    // Do Cargo.toml, pelo caminho que o cargo já resolveu para nós.
    let toml = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");

    let mut divergentes: Vec<String> = Vec::new();
    if toml != md {
        divergentes.push(format!("Cargo.toml = {toml}"));
    }

    // ⚠️ ESTE RAMO QUASE NUNCA DISPARA, e dizer isso importa mais que tê-lo.
    //
    // Medido por reversão em 20/08: desalinhar SÓ o `Cargo.lock` **não** derruba o
    // build. O `cargo build` **reescreve o lock em silêncio** a partir do
    // `Cargo.toml` ANTES de chamar o `build.rs` — então a divergência nunca chega
    // aqui. Quem de fato guarda o lock é o **`--locked`**, que aborta com
    // `error: cannot update the lock file ... because --locked was passed` (exit
    // 101), e é o que o empacotamento e o CI devem usar.
    //
    // Fica como cinto-e-suspensório para os casos em que alguém leia o lock sem o
    // cargo ter passado por ele. Não é a guarda do lock; a guarda é `--locked`.
    // Prometer o contrário seria a mesma mentira do comentário que este arquivo
    // veio substituir.
    match fs::read_to_string(raiz.join("Cargo.lock")) {
        Ok(lock) => match versao_no_lock(&lock) {
            Some(v) if v != md => divergentes.push(format!("Cargo.lock = {v}")),
            Some(_) => {}
            // Não derruba: lock sem o nosso pacote é estado esquisito, e derrubar o
            // build por causa da VERIFICAÇÃO seria a prova mais frágil que o provado.
            None => println!("cargo:warning=prova do bump: não achei o pacote `anna` no Cargo.lock"),
        },
        Err(_) => println!("cargo:warning=prova do bump: Cargo.lock ausente; portador não conferido"),
    }

    if !divergentes.is_empty() {
        panic!(
            "\n\n  ✗ BUMP PELA METADE — version.md diz {md}, e {} discorda(m):\n      {}\n\n  \
             Os três portadores desta casa são `version.md`, `Cargo.toml` e `Cargo.lock`.\n  \
             Aconteceu de verdade na 0.11.2: o binário saiu dizendo 0.11.1 com código da 0.11.2,\n  \
             e o app mostra esse número ao usuário no \"Sobre → Motor do Code\".\n\n  \
             Conserte o portador acima (não o version.md, que é a fonte) e compile de novo.\n",
            if divergentes.len() == 1 { "ele" } else { "eles" },
            divergentes.join("\n      "),
        );
    }
}
