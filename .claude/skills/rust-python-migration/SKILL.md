---
name: Rust <-> Python Project Migration
description: Migrate projects between Rust and Python with API compatibility and idiomatic patterns.
---

# Rust ↔ Python Project Migration

You are an expert polyglot software engineer specializing in seamless migrations between Rust and Python, with deep knowledge of language ecosystems, idiomatic patterns, and API compatibility preservation.

## Trigger

Invoke this skill when the user:
- Asks to migrate/convert a Rust project to Python
- Asks to migrate/convert a Python project to Rust
- Requests rewriting backend/service in Python/Rust
- Mentions "convert to Python/Rust", "migrate to Python/Rust", or "rewrite in Python/Rust"
- Asks to evaluate migration feasibility between Rust and Python

## Core Principles

1. **API Compatibility First**: External systems must not notice breaking changes
2. **Thorough Investigation**: Understand every feature before translating
3. **Modern Stack**: Always use latest stable versions of libraries/frameworks
4. **Idiomatic Code**: Write natural code in the target language, not transliterated code
5. **Safety**: Be extremely careful with data handling, types, and error propagation
6. **Python venv**: Always create and work inside virtual environment for Python projects

## Migration Strategy

### Phase 1: Project Analysis & Planning

#### 1.1 Project Discovery
- Identify project type (web server, CLI, library, service, etc.)
- Determine current language (Rust or Python)
- Map project structure and architecture
- Identify entry points and public APIs
- Document external dependencies (databases, APIs, file systems)

**Actions:**
```bash
# For Rust projects
ls -la
cat Cargo.toml
cat Cargo.lock
find src -name "*.rs" | head -20

# For Python projects
ls -la
cat requirements.txt || cat pyproject.toml || cat setup.py
cat poetry.lock || cat Pipfile.lock || ls venv/
find . -name "*.py" | head -20
```

#### 1.2 Feature Inventory
Create comprehensive list of:
- Core functionality and business logic
- API endpoints (REST, GraphQL, gRPC)
- Data models and schemas
- Authentication/authorization mechanisms
- Database operations and queries
- File I/O operations
- Concurrency patterns (async, threads, processes)
- External service integrations
- Configuration management
- Logging and error handling
- Testing approach

**Use Task tool with Explore agent** to systematically catalog features:
```
- Search for route/endpoint definitions
- Identify data structures and models
- Find database query patterns
- Locate authentication middleware
- Map service dependencies
```

#### 1.3 Dependency Analysis
For each dependency, find equivalent in target language:

**Rust → Python Common Mappings:**
- `axum` → `fastapi` or `flask`
- `tokio` → `asyncio` or `trio`
- `serde` → `pydantic` or `dataclasses`
- `sqlx` → `sqlalchemy` or `asyncpg`
- `diesel` → `sqlalchemy`
- `reqwest` → `httpx` or `aiohttp`
- `anyhow` → built-in exceptions + `traceback`
- `thiserror` → custom exception classes
- `tracing` → `loguru` or `structlog`
- `clap` → `click` or `typer`
- `chrono` → `datetime` or `pendulum`
- `uuid` → `uuid`
- `bcrypt` → `bcrypt` or `passlib`
- `jsonwebtoken` → `pyjwt`
- `regex` → `re` or `regex`
- `rusqlite` → `sqlite3` or `aiosqlite`

**Python → Rust Common Mappings:**
- `fastapi` → `axum` or `actix-web`
- `flask` → `axum` or `rocket`
- `django` → `axum` + custom ORM or `diesel`
- `asyncio` → `tokio`
- `pydantic` → `serde` + validation crates
- `sqlalchemy` → `sqlx` or `diesel`
- `requests` → `reqwest`
- `httpx` → `reqwest`
- `click` → `clap`
- `datetime` → `chrono`
- `logging` → `tracing` or `log`

**Action:** Create dependency mapping table with versions:
```
| Original | Version | Target | Latest Version | Notes |
|----------|---------|--------|----------------|-------|
| axum     | 0.8.0   | fastapi| 0.115.0        | Async web framework |
```

#### 1.4 API Surface Documentation
Document all public interfaces:
- HTTP endpoints (method, path, request/response schemas)
- Function signatures for libraries
- CLI commands and arguments
- Environment variables
- Configuration files
- Data formats (JSON, MessagePack, etc.)

**Critical:** This documentation becomes the compatibility contract.

### Phase 2: Environment Setup

#### 2.1 For Python Target (Rust → Python)

**MANDATORY: Always create venv and work inside it**

```bash
# Navigate to target directory
cd /path/to/project

# Create virtual environment
python -m venv venv

# Activate venv (Windows)
venv\Scripts\activate

# Activate venv (Linux/Mac)
source venv/bin/activate

# Upgrade pip
pip install --upgrade pip

# Verify venv is active
which python  # Should point to venv/bin/python
pip --version # Should reference venv
```

**Create project structure:**
```bash
mkdir -p src/{config,models,services,handlers,middleware,utils}
touch src/__init__.py
touch src/main.py
```

**Initialize pyproject.toml:**
```toml
[project]
name = "project-name"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = []

[build-system]
requires = ["setuptools>=68.0"]
build-backend = "setuptools.build_meta"
```

#### 2.2 For Rust Target (Python → Rust)

```bash
# Create new Cargo project
cargo new project-name --name project_name
cd project-name

# Initialize with workspace if needed
# Edit Cargo.toml
```

**Basic Cargo.toml:**
```toml
[package]
name = "project_name"
version = "0.1.0"
edition = "2021"

[dependencies]
```

### Phase 3: Core Migration

#### 3.1 Type System Translation

**Rust → Python:**
```rust
// Rust
struct User {
    id: i32,
    name: String,
    email: Option<String>,
    created_at: DateTime<Utc>,
}
```

```python
# Python (Pydantic)
from pydantic import BaseModel, EmailStr
from datetime import datetime
from typing import Optional

class User(BaseModel):
    id: int
    name: str
    email: Optional[EmailStr] = None
    created_at: datetime
```

**Python → Rust:**
```python
# Python
from dataclasses import dataclass
from typing import Optional

@dataclass
class User:
    id: int
    name: str
    email: Optional[str] = None
```

```rust
// Rust
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize)]
struct User {
    id: i32,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
}
```

#### 3.2 Async/Concurrency Patterns

**Rust (Tokio) → Python (asyncio):**
```rust
// Rust
async fn fetch_user(id: i32) -> Result<User, Error> {
    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = ?", id)
        .fetch_one(&pool)
        .await?;
    Ok(user)
}
```

```python
# Python
async def fetch_user(id: int) -> User:
    async with pool.acquire() as conn:
        result = await conn.fetchrow("SELECT * FROM users WHERE id = $1", id)
        if not result:
            raise UserNotFoundError(f"User {id} not found")
        return User(**result)
```

**Python (asyncio) → Rust (Tokio):**
```python
# Python
async def process_batch(items: list[Item]) -> list[Result]:
    tasks = [process_item(item) for item in items]
    return await asyncio.gather(*tasks)
```

```rust
// Rust
async fn process_batch(items: Vec<Item>) -> Vec<Result> {
    let tasks: Vec<_> = items
        .into_iter()
        .map(|item| tokio::spawn(process_item(item)))
        .collect();

    let results = futures::future::join_all(tasks).await;
    results.into_iter().filter_map(|r| r.ok()).collect()
}
```

#### 3.3 Error Handling Translation

**Rust → Python:**
```rust
// Rust
use anyhow::{Context, Result};

fn read_config() -> Result<Config> {
    let contents = std::fs::read_to_string("config.toml")
        .context("Failed to read config file")?;
    let config: Config = toml::from_str(&contents)
        .context("Failed to parse config")?;
    Ok(config)
}
```

```python
# Python
class ConfigError(Exception):
    """Configuration error"""
    pass

def read_config() -> Config:
    try:
        with open("config.toml", "r") as f:
            contents = f.read()
    except IOError as e:
        raise ConfigError(f"Failed to read config file: {e}") from e

    try:
        config = toml.loads(contents)
        return Config(**config)
    except Exception as e:
        raise ConfigError(f"Failed to parse config: {e}") from e
```

**Python → Rust:**
```python
# Python
def divide(a: float, b: float) -> float:
    if b == 0:
        raise ValueError("Cannot divide by zero")
    return a / b
```

```rust
// Rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MathError {
    #[error("Cannot divide by zero")]
    DivisionByZero,
}

fn divide(a: f64, b: f64) -> Result<f64, MathError> {
    if b == 0.0 {
        return Err(MathError::DivisionByZero);
    }
    Ok(a / b)
}
```

#### 3.4 Web Framework Migration

**Axum (Rust) → FastAPI (Python):**
```rust
// Rust (Axum)
async fn create_user(
    State(pool): State<PgPool>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<User>, StatusCode> {
    let user = sqlx::query_as!(...)
        .fetch_one(&pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(user))
}

let app = Router::new()
    .route("/users", post(create_user))
    .with_state(pool);
```

```python
# Python (FastAPI)
from fastapi import FastAPI, Depends, HTTPException, status
from sqlalchemy.ext.asyncio import AsyncSession

app = FastAPI()

@app.post("/users", response_model=User, status_code=status.HTTP_201_CREATED)
async def create_user(
    payload: CreateUserRequest,
    db: AsyncSession = Depends(get_db)
) -> User:
    try:
        # Create user logic
        user = await create_user_in_db(db, payload)
        return user
    except Exception as e:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=str(e)
        )
```

**FastAPI (Python) → Axum (Rust):**
```python
# Python (FastAPI)
@app.get("/users/{user_id}")
async def get_user(user_id: int, db: Session = Depends(get_db)):
    user = db.query(User).filter(User.id == user_id).first()
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    return user
```

```rust
// Rust (Axum)
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

async fn get_user(
    State(pool): State<PgPool>,
    Path(user_id): Path<i32>,
) -> Result<Json<User>, StatusCode> {
    let user = sqlx::query_as!(
        User,
        "SELECT * FROM users WHERE id = $1",
        user_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(user))
}

let app = Router::new()
    .route("/users/:user_id", get(get_user))
    .with_state(pool);
```

### Phase 4: Testing & Validation

#### 4.1 API Compatibility Testing

Create test suite to verify external API hasn't changed:

**For REST APIs:**
```python
# test_api_compatibility.py
import pytest
import httpx

BASE_URL = "http://localhost:8000"

@pytest.mark.asyncio
async def test_create_user_endpoint():
    async with httpx.AsyncClient() as client:
        payload = {"name": "Test User", "email": "test@example.com"}
        response = await client.post(f"{BASE_URL}/users", json=payload)

        # Verify status code
        assert response.status_code == 201

        # Verify response structure
        data = response.json()
        assert "id" in data
        assert data["name"] == "Test User"
        assert data["email"] == "test@example.com"
        assert "created_at" in data
```

```rust
// tests/api_compatibility.rs
#[tokio::test]
async fn test_create_user_endpoint() {
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "name": "Test User",
        "email": "test@example.com"
    });

    let response = client
        .post("http://localhost:8000/users")
        .json(&payload)
        .send()
        .await
        .unwrap();

    // Verify status code
    assert_eq!(response.status().as_u16(), 201);

    // Verify response structure
    let data: serde_json::Value = response.json().await.unwrap();
    assert!(data["id"].is_number());
    assert_eq!(data["name"].as_str().unwrap(), "Test User");
}
```

#### 4.2 Integration Testing

Test database operations, external services, file I/O:
- Verify data persistence
- Check transaction handling
- Validate error scenarios
- Test concurrent operations

#### 4.3 Performance Benchmarking

Compare before/after:
- Request latency (p50, p95, p99)
- Throughput (requests per second)
- Memory usage
- CPU utilization
- Startup time

**Tools:**
- `wrk` or `bombardier` for HTTP benchmarking
- `hyperfine` for CLI benchmarking
- Memory profilers (Valgrind, py-spy, memory_profiler)

### Phase 5: Deployment Preparation

#### 5.1 Dependencies Lock File

**Python:**
```bash
# Inside venv
pip freeze > requirements.txt

# Or with pip-tools
pip-compile pyproject.toml
```

**Rust:**
```bash
# Cargo.lock is automatically generated
cargo build
```

#### 5.2 Configuration Migration

Ensure environment variables, config files match:
```bash
# Compare configurations
diff original/.env migrated/.env
diff original/config.toml migrated/config.toml
```

#### 5.3 Docker Migration

**Rust → Python Dockerfile:**
```dockerfile
# Original Rust
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/app /usr/local/bin/
CMD ["app"]
```

```dockerfile
# Migrated Python
FROM python:3.12-slim
WORKDIR /app

# Copy requirements first for layer caching
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application
COPY src/ ./src/
COPY pyproject.toml .

CMD ["python", "-m", "uvicorn", "src.main:app", "--host", "0.0.0.0", "--port", "8000"]
```

#### 5.4 Documentation Updates

Update README with:
- New setup instructions
- New dependencies
- Changed environment variables
- Migration notes
- Breaking changes (if any)

## Execution Protocol

When this skill is invoked:

1. **Confirm direction**: "Migrating [Rust/Python] → [Python/Rust]"
2. **Execute Phase 1**: Analyze project thoroughly
   - Use Task tool with Explore agent for codebase understanding
   - Use Glob/Grep for finding patterns
   - Read key files (Cargo.toml, main.rs, requirements.txt, main.py)
3. **Present Migration Plan** to user:
   - List all features to migrate
   - Show dependency mapping table
   - Estimate complexity/risks
   - Ask for confirmation before proceeding
4. **Execute Phase 2**: Set up environment
   - **CRITICAL**: Create venv first if target is Python
5. **Execute Phase 3**: Migrate code module by module
   - Start with models/types
   - Then services/business logic
   - Then handlers/controllers
   - Then middleware
   - Finally main.rs/main.py
6. **Execute Phase 4**: Test thoroughly
   - Run compatibility tests
   - Compare API responses
   - Verify edge cases
7. **Execute Phase 5**: Prepare for deployment
8. **Final Report**: Summarize changes, risks, next steps

## Migration Order (Typical)

For web applications, follow this sequence:

1. **Data Models** (structs/classes)
2. **Database Layer** (queries, connections)
3. **Business Logic** (services, core functions)
4. **API Handlers** (routes, controllers)
5. **Middleware** (auth, logging, CORS)
6. **Configuration** (environment, settings)
7. **Entry Point** (main.rs/main.py)
8. **Tests**
9. **Documentation**
10. **CI/CD** (GitHub Actions, etc.)

## Critical Considerations

### Performance Implications

**Rust → Python:**
- Expect 5-20x slower performance (depending on workload)
- Benefits: Faster development, easier maintenance, richer ecosystem
- Mitigation: Use async I/O, caching, connection pooling

**Python → Rust:**
- Expect 5-50x faster performance
- Benefits: Memory safety, concurrency, deployment simplicity (single binary)
- Tradeoff: Steeper learning curve, longer compile times

### Type Safety

**Rust → Python:**
- Use Pydantic for runtime validation
- Enable mypy strict mode for static type checking
- Use type hints everywhere

**Python → Rust:**
- Leverage Rust's type system fully
- Use newtype pattern for domain types
- Implement validation at deserialization boundaries

### Error Handling Philosophy

**Rust:** Explicit, compile-time checked (Result<T, E>)
**Python:** Exception-based, runtime errors

Ensure error messages and status codes remain consistent!

## Safety Checklist

Before declaring migration complete:

- [ ] All API endpoints return identical response structures
- [ ] Error responses have same status codes and formats
- [ ] Database queries produce identical results
- [ ] Authentication/authorization behavior unchanged
- [ ] File paths and environment variables documented
- [ ] Configuration migration guide written
- [ ] Integration tests pass
- [ ] Performance benchmarks reviewed
- [ ] Dependencies locked (requirements.txt or Cargo.lock)
- [ ] Docker builds successfully
- [ ] README updated with new setup instructions
- [ ] No breaking changes to external consumers

## Common Pitfalls & Solutions

### 1. Integer Overflow
**Problem:** Python has arbitrary precision ints, Rust has fixed-size
**Solution:** Use appropriate Rust types (i64, u64) or validate ranges

### 2. Null/None Handling
**Problem:** Different null semantics
**Solution:** Always use Option<T> in Rust, Optional[T] in Python

### 3. String Encoding
**Problem:** Python strings are Unicode, Rust has String and &str
**Solution:** Be explicit about UTF-8, handle encoding errors

### 4. Async Runtime Differences
**Problem:** Tokio vs asyncio have different semantics
**Solution:** Test concurrent operations thoroughly, understand cancellation

### 5. Database Connection Pooling
**Problem:** Different pool management strategies
**Solution:** Match pool sizes and timeout configurations

### 6. Serialization Differences
**Problem:** JSON serialization may differ (dates, floats, nulls)
**Solution:** Test serialization explicitly, use compatible formats

## Python venv Best Practices

**MANDATORY for all Python migrations:**

1. **Always activate venv before any pip/python commands**
2. **Verify venv is active**: `which python` should show venv path
3. **Never install packages globally**: Use venv for isolation
4. **Include venv in .gitignore**: Don't commit venv directory
5. **Document venv setup**: Include in README
6. **Use requirements.txt**: Lock dependencies with `pip freeze`

```bash
# Correct workflow
python -m venv venv
source venv/bin/activate  # or venv\Scripts\activate on Windows
pip install fastapi uvicorn
pip freeze > requirements.txt
python -m uvicorn src.main:app

# Incorrect workflow (DON'T DO THIS)
pip install fastapi  # Installing globally, no venv!
python -m uvicorn src.main:app
```

## Final Notes

- **Migration is not transliteration**: Write idiomatic code in target language
- **Maintain API contracts**: External systems must not break
- **Test exhaustively**: Compatibility testing is non-negotiable
- **Document everything**: Future maintainers will thank you
- **Consider hybrid approaches**: Sometimes FFI (PyO3, cbindgen) is better than full migration
- **Respect user's requirements**: If they say migrate, migrate completely

---

## Example Output Format

After migration, provide report:

```markdown
## Migration Summary: [Project Name] (Rust → Python)

### Migrated Features
- ✅ User authentication (JWT)
- ✅ CRUD endpoints for users
- ✅ PostgreSQL integration
- ✅ File upload handling
- ✅ CORS middleware

### Dependency Mapping
| Rust Crate | Version | Python Package | Version |
|------------|---------|----------------|---------|
| axum       | 0.8.0   | fastapi        | 0.115.0 |
| sqlx       | 0.8.0   | asyncpg        | 0.29.0  |
| tokio      | 1.40.0  | asyncio        | (stdlib)|

### API Compatibility Status
- ✅ All endpoints match original behavior
- ✅ Response schemas identical
- ✅ Error codes preserved
- ⚠️  Performance: 8x slower (acceptable for this use case)

### Testing Results
- ✅ 45/45 integration tests passing
- ✅ API compatibility tests passing
- ✅ Load test: 500 req/s sustained

### Deployment Notes
- Python 3.12+ required
- Virtual environment setup: `python -m venv venv && source venv/bin/activate`
- Install: `pip install -r requirements.txt`
- Run: `uvicorn src.main:app --host 0.0.0.0 --port 8000`

### Next Steps
1. Deploy to staging environment
2. Run smoke tests with real traffic
3. Monitor performance metrics
4. Gradual rollout with canary deployment
```

---

**Remember**: External systems should not notice the migration happened. That's the ultimate success metric.

