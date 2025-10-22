# Testing Methodology

This document describes the testing philosophy and practices used in the `gdl` project to achieve **high code coverage with minimal test maintenance burden**.

## Philosophy: Test What Matters

**Goal**: Maintain ~20-25% coverage with ~100-120 unit tests

We prioritize testing **pure business logic** over infrastructure code. This gives us:
- ✅ High confidence in critical functionality
- ✅ Fast test execution (< 1 second)
- ✅ Easy test maintenance
- ✅ Clear test failures when logic breaks

**Not all code needs tests.** We focus on the code that benefits most from testing.

## What to Test (High Value)

### ✅ Pure Functions
Functions with no I/O, side effects, or external dependencies.

**Examples:**
- Strategy selection logic
- Mathematical calculations
- String/path manipulation
- Command builders
- Header parsing

**Why:** Easy to test, deterministic, often contains complex logic that's easy to get wrong.

### ✅ Business Logic
Decision-making code that determines application behavior.

**Examples:**
- Download strategy validation (`download/validation.rs`)
- Progress calculation (`download/calculations.rs`)
- Cache TTL logic (`http/headers.rs`)

**Why:** This is where bugs have the most impact on user experience.

### ✅ Data Transformation
Code that converts between data structures.

**Examples:**
- Building download tasks from GitHub content
- Filtering and categorizing items
- URL parsing and validation

**Why:** Easy to test with input/output pairs, often has edge cases.

## What NOT to Test (Low Value)

### ❌ I/O-Heavy Code
Code that primarily interacts with file systems, networks, or processes.

**Examples:**
- `download/file.rs` - File downloading with resume
- `git/sparse.rs` - Git operations via subprocess
- `main.rs` - Application entry point

**Why:** Requires mocking/integration tests, slow, brittle. Better covered by integration/smoke tests.

### ❌ Simple Glue Code
Code that just wires components together with no logic.

**Examples:**
- Module exports (`pub use`)
- Simple delegators
- Trait implementations with no logic

**Why:** No value - if the components work, the glue works.

### ❌ UI/Progress Code
Progress bars, formatters, display logic.

**Examples:**
- `progress.rs` - Progress bar management
- Log formatting

**Why:** Hard to test, subjective, low bug impact.

### ❌ External Dependencies
Code that depends on external services/state.

**Examples:**
- `update/manager.rs` - Self-update via external crate
- `rate_limit.rs` - GitHub API rate tracking (partially tested)

**Why:** Requires mocking external state, tests become integration tests.

## Testing Techniques

### 1. Consolidate Related Assertions

**❌ Bad: One assertion per test**
```rust
#[test]
fn test_is_cacheable_status_200() {
    assert!(is_cacheable(200, None));
}

#[test]
fn test_is_cacheable_status_304() {
    assert!(is_cacheable(304, None));
}

#[test]
fn test_is_cacheable_status_404() {
    assert!(!is_cacheable(404, None));
}
```

**✅ Good: Group related scenarios**
```rust
#[test]
fn test_is_cacheable() {
    // Cacheable responses
    assert!(is_cacheable(200, None));
    assert!(is_cacheable(304, None));

    // Not cacheable
    assert!(!is_cacheable(404, None));
    assert!(!is_cacheable(200, Some("no-store")));
}
```

**Benefit:** 3 tests → 1 test, same coverage, clearer intent.

### 2. Test Behavior, Not Implementation

**❌ Bad: Testing internal details**
```rust
#[test]
fn test_calculate_progress_calls_division() { ... }

#[test]
fn test_calculate_progress_multiplies_by_100() { ... }
```

**✅ Good: Testing outcomes**
```rust
#[test]
fn test_calculate_progress_percentage() {
    assert_eq!(calculate_progress_percentage(50, Some(100)), Some(50.0));
    assert_eq!(calculate_progress_percentage(50, None), None);
}
```

**Benefit:** Tests survive refactoring, focus on contracts.

### 3. Use Table-Driven Approaches

**✅ Good: Multiple cases in one test**
```rust
#[test]
fn test_select_optimal_strategy() {
    // Without git: use Zip for large counts
    assert_eq!(select_optimal_strategy(false, false, 150), DownloadStrategy::Zip);
    assert_eq!(select_optimal_strategy(false, false, 50), DownloadStrategy::Api);

    // With git: whole repo uses Git
    assert_eq!(select_optimal_strategy(true, true, 1000), DownloadStrategy::Git);
    assert_eq!(select_optimal_strategy(true, false, 100), DownloadStrategy::Zip);

    // Boundary cases
    assert_eq!(select_optimal_strategy(false, false, 101), DownloadStrategy::Zip);
}
```

**Benefit:** Clear documentation of all behaviors in one place.

### 4. Focus on Edge Cases + Happy Path

Don't test every possible input - test the extremes and the common case.

**✅ Essential test cases:**
- Happy path (normal input)
- Empty/None/zero values
- Boundary conditions (50 vs 51 files)
- Error conditions

**❌ Skip:**
- Every number between 1-100
- Every possible string permutation
- Redundant variations

### 5. Avoid Redundant Tests

If behavior is already tested through another function, don't re-test it.

**Example:** If `build_download_tasks()` calls `build_download_task()` in a loop, test `build_download_task()` thoroughly and `build_download_tasks()` lightly.

## Test Structure Guidelines

### Group by Module
Keep tests in the same file as the code they test:
```rust
// src/download/validation.rs
pub fn validate_download_strategy(...) -> DownloadStrategy { ... }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_download_strategy() { ... }
}
```

### Clear Test Names
Use descriptive names that explain what's being tested:

**✅ Good:**
```rust
#[test]
fn test_calculate_progress_percentage() { ... }

#[test]
fn test_is_cacheable() { ... }
```

**❌ Bad:**
```rust
#[test]
fn test_1() { ... }

#[test]
fn test_function_returns_correct_value() { ... }
```

### Inline Comments for Complex Scenarios
Add comments to clarify test intent:

```rust
#[test]
fn test_can_resume_download() {
    // Can resume
    assert!(can_resume_download(1024, Some(2048), true));

    // Cannot resume - various reasons
    assert!(!can_resume_download(1024, Some(2048), false)); // No range support
    assert!(!can_resume_download(0, Some(2048), true));     // No partial data
    assert!(!can_resume_download(2048, Some(2048), true));  // Already complete
}
```

## Coverage Targets by Module Type

| Module Type | Target Coverage | Test Count |
|-------------|----------------|------------|
| Pure logic (validation, calculations) | **100%** | 4-6 tests |
| Data transformation (task builders) | **80-90%** | 3-5 tests |
| Command builders | **80-90%** | 6-10 tests |
| Parser/validators | **80-90%** | 5-8 tests |
| I/O operations | **0-20%** | 0-2 tests |
| Application glue | **0%** | 0 tests |

## When to Add Tests

### ✅ Add tests when:
- Implementing pure functions with complex logic
- Adding new validation or decision-making code
- Fixing a bug (add test to prevent regression)
- Building reusable utilities

### ⚠️ Consider carefully:
- I/O-heavy operations (integration test instead?)
- Simple CRUD operations
- Third-party library wrappers

### ❌ Don't add tests for:
- Module exports
- Simple type definitions
- Obvious delegations
- Code that's better tested via integration tests

## Running Tests

```bash
# Run all tests
cargo test

# Run tests for a specific module
cargo test validation
cargo test calculations

# Run tests with coverage report
cargo tarpaulin --out Stdout --skip-clean --exclude-files 'tests/*'

# Watch mode (requires cargo-watch)
cargo watch -x test
```

## Reviewing Test PRs

When reviewing test additions, ask:

1. **Is this testing pure logic or I/O?** (Pure = good candidate)
2. **Could this test be consolidated with others?** (Fewer tests = better)
3. **Does this test behavior or implementation?** (Behavior = better)
4. **Is the test name clear?** (Should explain what's tested)
5. **Are edge cases covered?** (Empty, None, boundaries)

## Examples from Codebase

### ✅ Great Example: `download/validation.rs`
- 4 tests for 25 lines = 100% coverage
- Tests strategy selection logic (pure function)
- Consolidates related scenarios
- Clear test names

```rust
#[test]
fn test_validate_download_strategy() {
    // Auto mode selects based on conditions
    assert_eq!(validate_download_strategy(DownloadStrategy::Auto, true, ...), DownloadStrategy::Git);

    // Explicit strategies unchanged
    assert_eq!(validate_download_strategy(DownloadStrategy::Api, ...), DownloadStrategy::Api);
}
```

### ✅ Great Example: `http/headers.rs`
- 6 tests for 36 lines = 100% coverage
- Tests header parsing (pure function)
- Groups extraction, validation, and caching logic

### ❌ Intentionally NOT Tested: `download/file.rs`
- 0 tests for 104 lines = 0% coverage
- Heavy I/O: file downloads, network requests, resume logic
- Better tested via integration tests or manual testing
- Would require complex mocking to unit test

## Maintenance

### When Coverage Drops Below 20%
This likely means someone added pure logic without tests. Review recent changes.

### When Test Count Grows Above 150
Time to consolidate. Look for:
- Multiple tests for the same function
- Tests that can be combined with table-driven approach
- Redundant edge case testing

### When Tests Become Slow (>2 seconds)
Tests probably contain I/O. Either:
- Move to integration test suite
- Simplify to test pure logic only
- Remove if not valuable

## Summary

**Principles:**
1. Test pure logic thoroughly (100% coverage)
2. Test I/O sparingly (0-20% coverage)
3. Consolidate related tests
4. Focus on behavior over implementation
5. Keep total test count < 150

**Result:**
- ~20-25% overall coverage
- ~100-120 unit tests
- Fast test execution
- High confidence in critical logic
- Low maintenance burden

**Remember:** The goal is not maximum coverage - it's maximum confidence with minimum maintenance cost.
