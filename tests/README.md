# HandsOff Test Suite

This directory contains the test suite for HandsOff Input Lock.

## Running Tests

### Run all tests
```bash
cargo test
```

### Run specific test file
```bash
cargo test --test auth_tests
cargo test --test app_state_tests
cargo test --test keycode_tests
```

### Run specific test
```bash
cargo test test_hash_passphrase
cargo test test_buffer_operations
```

### Run with output
```bash
cargo test -- --nocapture
```

### Run with multiple threads
```bash
cargo test -- --test-threads=4
```

## Test Coverage

### ✅ Unit Tests (Safe to run anytime)

#### Authentication (`auth_tests.rs`)
- Passphrase hashing (SHA-256)
- Passphrase verification
- Hash determinism
- Unicode and edge cases

#### Application State (`app_state_tests.rs`)
- Lock/unlock state management
- Input buffer operations
- Auto-lock timing
- Buffer reset timing
- Thread safety
- Talk key state

#### Keycode Conversion (`keycode_tests.rs`)
- Letter key mapping (a-z, A-Z)
- Number key mapping (0-9, !@#$%^&*())
- Punctuation and special characters
- Shift key modifiers
- Invalid keycode handling

### ⚠️ Integration Tests (Require special setup)

Integration tests that interact with system APIs are marked with `#[ignore]` and must be run explicitly:

```bash
cargo test -- --ignored
```

**Note**: Integration tests require:
- Accessibility permissions granted
- macOS environment
- Potential system interaction

### ❌ Manual Tests (Cannot be automated)

The following must be tested manually (see `docs/SAFE-DEVELOPMENT.md`):

- Event tap blocking behavior
- Menu bar UI interaction
- Touch ID authentication
- Notification display
- Actual input blocking during lock
- Video conferencing compatibility

## Test Organization

```
tests/
├── README.md           # This file
├── auth_tests.rs       # Authentication and cryptography tests
├── app_state_tests.rs  # Application state management tests
└── keycode_tests.rs    # Keycode to character conversion tests
```

## Adding New Tests

### Unit Test Template
```rust
#[test]
fn test_description() {
    // Arrange
    let input = "test_data";

    // Act
    let result = function_to_test(input);

    // Assert
    assert_eq!(result, expected_value);
}
```

### Integration Test Template
```rust
#[test]
#[ignore] // Must be run explicitly
fn integration_test_description() {
    // Setup
    // ... test code ...
    // Cleanup
}
```

## CI/CD Integration

For continuous integration, run only unit tests (skip integration):

```bash
cargo test --lib --bins --tests -- --skip integration
```

Or run all non-ignored tests:

```bash
cargo test
```

## Test Safety

**IMPORTANT**: Unit tests are safe to run and do NOT interact with system input blocking. They test pure logic and state management.

For testing actual input blocking behavior, see:
- `docs/SAFE-DEVELOPMENT.md` - Safe development practices
- `specs/phase-2.md` - Testing checklist

## Test Data

Tests use deterministic data and do not:
- Access the system keychain (tests use mock data)
- Create event taps (requires permissions)
- Block actual input
- Interact with the menu bar

## Troubleshooting

### Tests hang
- Check for deadlocks in thread safety tests
- Reduce timeout durations in timing tests

### Tests fail intermittently
- Timing tests may be flaky on slow machines
- Increase sleep durations slightly

### "Permission denied" errors
- Integration tests require Accessibility permissions
- Run with `cargo test` (not `--ignored`) to skip them

## Future Test Coverage

Planned additions:
- Keychain integration tests (with mock keychain)
- Hotkey manager unit tests
- Settings persistence tests
- Error handling tests
- Performance benchmarks

See `specs/phase-2.md` section 5.3 for complete testing roadmap.
