//! Stopping a turn without killing the process (0.14.0).
//!
//! Until 0.13.10 nothing handled SIGINT during a turn: Ctrl-C ended `anna` itself, the REPL
//! lost the turn's history (the session was saved only after the turn), and a running `bash`
//! child kept going on its own, because it runs in its own process group. A host had no way to
//! stop a turn short of killing the sidecar. Hermes and claw-code both interrupt the turn and
//! keep the transcript valid (comparison of 02/11/2026).
//!
//! One flag, raised by Ctrl-C in the terminal or by `{"type":"cancel"}` from an NDJSON host,
//! and checked where the turn waits: the stream, the retry wait, between tool calls, inside
//! `bash` (which then kills its process group), and while a host decides a gate or runs a
//! command. The agent loop then closes the turn with a valid history (see `agent::run_turn`).
//!
//! The same reader takes `{"type":"credential"}` lines (0.16.0): a new gateway key, applied at
//! once — see `credencial`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// A stop was asked for the turn in progress (or for the host's turn already queued).
static PEDIDO: AtomicBool = AtomicBool::new(false);
/// A turn is running (`Turno` guard).
static EM_TURNO: AtomicBool = AtomicBool::new(false);
/// The last turn that ended was interrupted: the planning pipeline stops on it.
static ULTIMO_INTERROMPIDO: AtomicBool = AtomicBool::new(false);
/// `user` lines the host sent that no turn has taken yet (see `ler_stdin`).
static PENDENTES: AtomicUsize = AtomicUsize::new(0);

/// How often a blocking wait looks at the flag. Short enough to feel immediate, long enough
/// not to spin.
const OLHAR_A_CADA: Duration = Duration::from_millis(50);

pub fn pedir() {
    PEDIDO.store(true, Ordering::SeqCst);
}

pub fn pedido() -> bool {
    PEDIDO.load(Ordering::SeqCst)
}

/// Resolves when a stop is asked. For `tokio::select!` against the stream and the retry wait.
pub async fn esperar() {
    while !pedido() {
        tokio::time::sleep(OLHAR_A_CADA).await;
    }
}

pub fn ultimo_turno_interrompido() -> bool {
    ULTIMO_INTERROMPIDO.load(Ordering::SeqCst)
}

/// Marks a turn as running. The flag is NOT cleared here: a host may have cancelled the very
/// turn it had just queued, before this guard existed. It is cleared when the turn ends.
pub struct Turno;

impl Turno {
    pub fn comecar() -> Self {
        ULTIMO_INTERROMPIDO.store(false, Ordering::SeqCst);
        EM_TURNO.store(true, Ordering::SeqCst);
        Turno
    }
}

impl Drop for Turno {
    fn drop(&mut self) {
        EM_TURNO.store(false, Ordering::SeqCst);
        ULTIMO_INTERROMPIDO.store(PEDIDO.swap(false, Ordering::SeqCst), Ordering::SeqCst);
    }
}

/// Terminal: the first Ctrl-C during a turn stops the turn; a second one, or a Ctrl-C when no
/// turn is running, ends `anna` as before (exit 130). The second is the way out of anything
/// that does not look at the flag. At the REPL prompt, `rustyline` reads Ctrl-C as a key, not
/// as a signal, so this handler does not change how the prompt itself exits.
pub fn instalar_ctrl_c(rt: &tokio::runtime::Runtime) {
    rt.spawn(async {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            if !EM_TURNO.load(Ordering::SeqCst) || pedido() {
                eprintln!();
                std::process::exit(130);
            }
            pedir();
            eprintln!("\n⏹ interrompendo o turno… (Ctrl-C de novo encerra o anna)");
        }
    });
}

/// What the NDJSON host sent, read by one thread so a `cancel` arrives while the turn is
/// streaming or running a tool. Every other line is queued in order for whoever reads next:
/// the main loop between turns, a gate waiting for its decision, or the host runner waiting
/// for a command result.
fn host() -> &'static Mutex<Receiver<String>> {
    static RX: OnceLock<Mutex<Receiver<String>>> = OnceLock::new();
    RX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            loop {
                line.clear();
                if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                    return; // EOF: dropping `tx` tells the readers
                }
                let valor = serde_json::from_str::<serde_json::Value>(line.trim()).ok();
                // A new gateway key (0.16.0) is applied here, at once, and never queued: a turn
                // or a gate waiting on the queue must not see it, and nothing downstream may log
                // or echo it (`credencial`).
                if let Some(chave) = valor.as_ref().and_then(crate::credencial::da_linha) {
                    crate::credencial::trocar(chave);
                    continue;
                }
                let tipo = valor.and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string));
                match tipo.as_deref() {
                    Some("cancel") => {
                        // Honored for the turn in progress, or for a `user` turn already
                        // queued; sent when nothing is running or queued, it is ignored.
                        if EM_TURNO.load(Ordering::SeqCst) || PENDENTES.load(Ordering::SeqCst) > 0 {
                            pedir();
                        }
                        continue;
                    }
                    // Leaving also stops the turn in progress; the line still goes through
                    // so the main loop ends after it.
                    Some("exit") if EM_TURNO.load(Ordering::SeqCst) => pedir(),
                    Some("user") => {
                        PENDENTES.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }
                if tx.send(line.clone()).is_err() {
                    return;
                }
            }
        });
        Mutex::new(rx)
    })
}

/// One line from the host.
pub enum Linha {
    Texto(String),
    /// stdin closed.
    Fim,
    /// A stop was asked while waiting (only from `linha_do_host_no_turno`).
    Interrompido,
}

fn receber(espera: Option<Duration>) -> Result<String, RecvTimeoutError> {
    let rx = host().lock().unwrap_or_else(|e| e.into_inner());
    let r = match espera {
        Some(d) => rx.recv_timeout(d),
        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
    };
    if let Ok(line) = &r {
        if line.contains("\"user\"")
            && serde_json::from_str::<serde_json::Value>(line.trim())
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(|t| t == "user"))
                .unwrap_or(false)
        {
            PENDENTES.fetch_sub(1, Ordering::SeqCst);
        }
    }
    r
}

/// Between turns: blocks until the next line.
pub fn linha_do_host() -> Linha {
    match receber(None) {
        Ok(l) => Linha::Texto(l),
        Err(_) => Linha::Fim,
    }
}

/// Inside a turn (a gate or a host-run command): also returns when a stop is asked.
pub fn linha_do_host_no_turno() -> Linha {
    loop {
        if pedido() {
            return Linha::Interrompido;
        }
        match receber(Some(OLHAR_A_CADA)) {
            Ok(l) => return Linha::Texto(l),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Linha::Fim,
        }
    }
}
