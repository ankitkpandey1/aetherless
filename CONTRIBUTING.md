# Contributing to Aetherless

Thank you for your interest in contributing to Aetherless! This document provides guidelines and information for contributors.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## Getting Started

### Prerequisites

- Rust 1.70+
- Linux (for eBPF/XDP features)
- Python 3.8+ (for running examples)

### Setup

```bash
# Clone the repository
git clone https://github.com/yourusername/aetherless.git
cd aetherless

# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run clippy
cargo clippy --workspace
```

## Development Workflow

### Branch Naming

- `feature/description` - New features
- `fix/description` - Bug fixes
- `docs/description` - Documentation improvements
- `refactor/description` - Code refactoring

### Commit Messages

Use conventional commits:

```
feat: add shared memory ring buffer
fix: handle port conflict in registry
docs: update installation instructions
test: add integration tests for CRIU
refactor: simplify state machine transitions
```

### Pull Request Process

1. **Fork** the repository
2. **Create** a feature branch from `main`
3. **Write** code following our coding guidelines
4. **Add** tests for new functionality
5. **Run** the full test suite: `cargo test --workspace`
6. **Run** clippy: `cargo clippy --workspace`
7. **Submit** a pull request with a clear description

## Coding Guidelines

### Error Handling

- Use explicit enum error types (see `aetherless-core/src/error.rs`)
- No `Box<dyn Error>` or `anyhow::Result`
- All errors must be strongly typed

```rust
// Good
fn process() -> Result<(), AetherError> {
    Err(AetherError::FunctionNotFound(func_id))
}

// Bad
fn process() -> Result<(), Box<dyn std::error::Error>> { ... }
```

### Type Safety

- Use Newtype pattern for validated inputs
- Validate at construction time

```rust
// Good
let port = Port::new(8080)?;  // Validated

// Bad
let port: u16 = 8080;  // No validation
```

### No Fallbacks

- If a critical component fails, return an error
- Never silently degrade functionality

```rust
// Good
if ebpf_failed {
    return Err(AetherError::Ebpf(EbpfError::LoadFailed { ... }));
}

// Bad
if ebpf_failed {
    println!("Warning: eBPF failed, using fallback");
    use_userspace_routing();  // Silent degradation
}
```

### Testing

- Write unit tests for all public functions
- Add integration tests for cross-module functionality
- Aim for high coverage of error paths

```bash
# Run all tests
cargo test --workspace

# Run with output
cargo test --workspace -- --nocapture

# Run specific test
cargo test -p aetherless-core config::tests
```

## Project Structure

```
aetherless/
├── aetherless-core/       # Core library
│   └── src/
│       ├── config.rs      # YAML configuration
│       ├── criu/          # CRIU lifecycle
│       ├── error.rs       # Error types
│       ├── registry.rs    # Function registry
│       ├── shm/           # Shared memory
│       ├── state.rs       # State machine
│       └── types.rs       # Newtype wrappers
│
├── aetherless-cli/        # CLI tool
│   └── src/
│       ├── commands/      # CLI commands
│       ├── main.rs        # Entry point
│       └── tui/           # TUI dashboard
│
├── aetherless-ebpf/       # eBPF data plane
│   └── src/
│       └── main.rs        # XDP manager
│
└── examples/              # Example handlers
```

## Adding a New Feature

1. **Design** - Open an issue to discuss the design
2. **Types** - Add any new types to `types.rs`
3. **Errors** - Add error variants to `error.rs`
4. **Implementation** - Write the feature code
5. **Tests** - Add unit and integration tests
6. **Documentation** - Update README and code docs

## Reporting Issues

When reporting issues, please include:

- Rust version (`rustc --version`)
- OS and kernel version (`uname -a`)
- Steps to reproduce
- Expected vs actual behavior
- Error messages (full output)

## Documentation

- Update README.md for user-facing changes
- Add rustdoc comments for public APIs
- Update ARCHITECTURE.md for structural changes

## License

By contributing, you agree that your contributions will be licensed under the MIT License.

## Questions?

- Open a GitHub issue for questions
- Check existing issues before creating new ones

Thank you for contributing! 🚀
