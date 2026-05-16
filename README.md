# telomere

`telomere` is a high-performance, asynchronous Telegram media downloader written in Rust. It utilizes the `grammers` library to interface with the Telegram API and handles concurrent downloads efficiently. 

Designed with security and modern infrastructure practices in mind, `telomere` supports declarative deployment and encrypted secret management via **Agenix** on **NixOS**, ensuring your API credentials never leak into environment logs or public repositories.

## Features

- **Asynchronous Architecture:** Powered by `tokio` for fast, non-blocking I/O operations.
- **Granular Control:** Download media from specific users, channels, groups, or individual forum topics.
- **Secure Secret Handling:** Reads sensitive credentials (API ID, API Hash) securely from files at runtime rather than relying on insecure system environment variables.
- **NixOS Ready:** Native support for Nix flakes and declarative configuration management.

---

## Installation & Build Instructions

### Prerequisites
To build `telomere`, you need the Rust toolchain installed on your system.

```bash
# Clone the repository
git clone [https://github.com/yourusername/telomere.git](https://github.com/yourusername/telomere.git)
cd telomere
cargo build --release

```
### Running Locally

```bash
export TG_ID_FILE="/path/to/your/tg_id"
export TG_HASH_FILE="/path/to/your/tg_hash"

cargo run -- list
```
