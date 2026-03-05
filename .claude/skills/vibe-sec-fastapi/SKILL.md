---
name: Vibe-Security-Skill-FastAPI
description: Secure coding guide tailored for FastAPI backends (auth, validation, DB, uploads, headers). Use when implementing or reviewing FastAPI APIs.
---

# Vibe Security Skill (FastAPI / Backend)

Use this guide when writing or reviewing a **FastAPI** backend. Approach every change like a bug hunter: assume hostile inputs, malicious clients, and misconfigured deployments.

## Default Security Posture (Do This First)

- Prefer **deny-by-default** authorization (explicit allow rules).
- Validate and normalize **all inputs server-side** (Pydantic + explicit allowlists).
- Use **parameterized SQL** (never string concatenation).
- Keep auth **simple**: short-lived access tokens, clear revocation/rotation story.
- Avoid leaking internals: consistent error shapes, no stack traces to clients.

---

## AuthN/AuthZ (FastAPI Patterns)

### 1) "Current User" Dependency (Centralize Verification)

- Put token verification in one place (dependency), not ad-hoc in every route.
- Always **whitelist algorithms**; never trust the token header for algorithm selection.

```python
from fastapi import Depends, HTTPException, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from jose import JWTError, jwt

JWT_SECRET = "..."  # load from env
JWT_ALG = "HS256"

bearer = HTTPBearer(auto_error=False)

def get_current_user(
    creds: HTTPAuthorizationCredentials | None = Depends(bearer),
):
    if creds is None:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Not authenticated")

    token = creds.credentials
    try:
        payload = jwt.decode(token, JWT_SECRET, algorithms=[JWT_ALG])
    except JWTError:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid token")

    user_id = payload.get("sub")
    if not user_id:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid token")

    return {"id": user_id, "roles": payload.get("roles", [])}
```

### 2) Ownership Checks (Stop IDOR)

For every resource read/write:
- Load the resource by ID.
- If not found: return 404.
- If found but user lacks access: **return 404** (avoid enumeration) unless you intentionally prefer 403.

```python
from fastapi import HTTPException

def require_owner(resource_owner_id: str, current_user: dict) -> None:
    if resource_owner_id != current_user["id"]:
        raise HTTPException(status_code=404, detail="Not found")
```

### 3) Role Checks (Keep It Explicit)

- Do not accept `role` / `is_admin` / permissions from clients.
- Enforce server-side, ideally in a dependency.

```python
from fastapi import HTTPException, status

def require_role(current_user: dict, role: str) -> None:
    if role not in set(current_user.get("roles", [])):
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Forbidden")
```

---

## Input Validation (Pydantic)

### Mass Assignment Defense

Reject unknown fields to prevent privilege escalation via "extra" JSON keys.

```python
from pydantic import BaseModel, ConfigDict, EmailStr, Field

class UserUpdate(BaseModel):
    model_config = ConfigDict(extra="forbid")

    full_name: str | None = Field(None, max_length=120)
    email: EmailStr | None = None
```

### Validation Checklist

- [ ] `extra="forbid"` (or equivalent) on request models
- [ ] Max lengths on strings (avoid DOS via huge payloads)
- [ ] Constrained types for IDs (UUID, ints) and enums for finite sets
- [ ] Reject or bound lists (max items) and nested objects
- [ ] Validate query params (pagination bounds; sort allowlists)

---

## SQL Injection (MSSQL / pyodbc)

### Parameterized Queries (PRIMARY DEFENSE)

```python
# VULNERABLE (never do this)
sql = f"SELECT * FROM TA_Time_Attendance WHERE PID = {pid}"

# SECURE (parameterized)
sql = "SELECT * FROM TA_Time_Attendance WHERE PID = ?"
cursor.execute(sql, pid)
```

### ORDER BY / Column Names (Must Allowlists)

SQL parameters cannot safely replace identifiers. Use allowlists.

```python
ALLOWED_SORT = {"time_entry": "Time_Entry", "pid": "PID"}
sort_col = ALLOWED_SORT.get(sort_key)
if not sort_col:
    raise HTTPException(status_code=400, detail="Invalid sort")

sql = f"SELECT * FROM TA_Time_Attendance ORDER BY {sort_col} DESC"
cursor.execute(sql)
```

### Query Safety Checklist

- [ ] No string concatenation with user input
- [ ] Allowlist dynamic identifiers (ORDER BY, columns, table names)
- [ ] Bound pagination (limit/offset) to sane ranges
- [ ] Handle errors without returning raw SQL/driver messages

---

## File Uploads (FastAPI `UploadFile`)

### Common Issues

- Path traversal via filename
- Uploading HTML/SVG and later serving it as content (XSS)
- Overly large files (memory/disk DOS)
- MIME spoofing (Content-Type lies)

### Secure Handling Pattern

- Enforce max size server-side.
- Ignore client filename; store as random UUID.
- Validate magic bytes and (for images) try decoding with an image library.
- Store outside webroot; serve with safe headers.

```python
import io
import uuid
from fastapi import HTTPException, UploadFile

MAX_BYTES = 5 * 1024 * 1024

async def read_limited(upload: UploadFile) -> bytes:
    data = await upload.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise HTTPException(status_code=413, detail="File too large")
    return data

def safe_filename() -> str:
    return f"{uuid.uuid4()}.bin"
```

If accepting images, prefer verifying by decoding (example uses Pillow):

```python
from PIL import Image

def verify_image_bytes(data: bytes) -> None:
    try:
        Image.open(io.BytesIO(data)).verify()
    except Exception:
        raise HTTPException(status_code=400, detail="Invalid image")
```

---

## SSRF (When Fetching URLs Server-Side)

If any endpoint fetches a user-provided URL (webhooks, previews, imports):

- Prefer strict **allowlists** of domains.
- Resolve DNS and block private/internal ranges.
- Block cloud metadata IPs (e.g., `169.254.169.254`).
- Limit redirects, timeouts, and response size.

---

## CORS / CSRF (APIs)

- If you use `Authorization: Bearer ...` from mobile/web clients, CSRF risk is lower than cookie auth, but still:
  - Do not allow wildcard origins in production.
  - Do not set `allow_credentials=True` unless you actually use cookies.

FastAPI CORS baseline:

```python
from fastapi.middleware.cors import CORSMiddleware

app.add_middleware(
    CORSMiddleware,
    allow_origins=["https://your.app"],  # exact origins
    allow_credentials=False,
    allow_methods=["GET", "POST", "PUT", "DELETE"],
    allow_headers=["Authorization", "Content-Type"],
)
```

---

## Security Headers (API-Friendly Defaults)

Even for APIs, add defensive headers (especially if you also serve docs/static).

```python
from starlette.middleware.base import BaseHTTPMiddleware

class SecurityHeadersMiddleware(BaseHTTPMiddleware):
    async def dispatch(self, request, call_next):
        response = await call_next(request)
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["X-Frame-Options"] = "DENY"
        response.headers["Referrer-Policy"] = "strict-origin-when-cross-origin"
        return response
```

For HSTS (`Strict-Transport-Security`): only set it when you are fully HTTPS in production.

---

## JWT Security (Python-Specific Notes)

- Use a strong secret (256-bit random for HS256) or prefer RS256 with key rotation.
- Require `exp` and validate it.
- Consider `iss` / `aud` if you have multiple services/clients.
- Plan refresh-token rotation if you support long sessions.

Checklist:
- [ ] Algorithm allowlist on verify
- [ ] `exp` present and checked
- [ ] Secrets from env/secret manager (never in repo)
- [ ] Minimal claims (no PII)
- [ ] Clear logout story (revocation/short TTL)

---

## Logging & Error Handling

- Never log passwords, tokens, raw auth headers, or full request bodies by default.
- Prefer structured logs; redact sensitive keys (`password`, `token`, `authorization`).
- In production, return generic messages; keep details in server logs.

---

## Rate Limiting & Abuse Controls

Add rate limits to:
- Login / OTP / password reset
- File uploads
- Any expensive queries

Implementation can be:
- App middleware (e.g., `slowapi`) or
- Reverse proxy/API gateway (nginx/Cloudflare) (often simplest).

---

## Minimal Security Test Checklist (pytest)

- [ ] IDOR: user A cannot read/update user B resources (expects 404/403)
- [ ] JWT: invalid/expired token rejected
- [ ] Mass assignment: unknown fields rejected (`extra="forbid"`)
- [ ] Upload: oversize rejected; invalid image rejected
- [ ] SQL: sort/pagination allowlists enforced
