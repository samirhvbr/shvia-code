0.11.0

> ⚠️ **O `Cargo.toml` NÃO é propagado automaticamente** (este repo não tem
> `sync-version`). O commit 0.10.0 bumpou só este arquivo e deixou o manifesto
> em 0.9.0 — o binário saía se apresentando como `anna 0.9.0` e o
> `build-local.sh` do SHVIA-DESKTOP abortava no estágio D5, que exige piso
> 0.10.0 para não empacotar um Modo Code que trava no 422 em sessão longa
> (visto em 19/08). **Ao bumpar aqui, bumpe o `Cargo.toml` no mesmo commit** e
> confira com `cargo build --release && ./target/release/anna --version`.

