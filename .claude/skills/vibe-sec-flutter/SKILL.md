---
name: Vibe-Security-Skill-Flutter
description: Secure coding guide tailored for Flutter mobile apps integrating with an API (token storage, networking, logging, uploads, device risks). Use when implementing Flutter<->backend flows.
---

# Vibe Security Skill (Flutter / Frontend Integration)

Use this guide when building **Flutter** features that integrate with a backend (e.g., FastAPI). Assume the client device is hostile (rooted/jailbroken, reverse engineered), and that the network is monitored (MITM attempts). Your goal is to **reduce exposure** and **avoid creating new attack paths** -- the server remains the ultimate source of truth.

## Default Rules (Do This First)

- Never trust the client for authorization decisions: the server must validate everything.
- Store secrets/tokens in **platform secure storage**, not preferences or plain files.
- Use **HTTPS** everywhere; no "temporarily allow HTTP" exceptions in production.
- Avoid logging sensitive data (tokens, GPS, photos, PII), especially in release builds.
- Keep offline caches minimal; encrypt if it contains PII or auth material.

---

## Token & Secret Storage

### Use Secure Storage

- Use `flutter_secure_storage` (Keychain/Keystore) for access/refresh tokens.
- Do not store tokens in `SharedPreferences` / plaintext files.

```dart
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

class TokenStore {
  final FlutterSecureStorage _storage = const FlutterSecureStorage();

  Future<void> saveAccessToken(String token) =>
      _storage.write(key: 'access_token', value: token);

  Future<String?> readAccessToken() =>
      _storage.read(key: 'access_token');

  Future<void> clear() async {
    await _storage.delete(key: 'access_token');
  }
}
```

### Client-Side Secrets Reality Check

- Assume anything shipped in the app can be extracted (API keys, endpoints, feature flags).
- If a secret must remain secret, it must live server-side.

---

## Networking (API Integration)

### Baseline Requirements

- Enforce HTTPS.
- Set timeouts (connect/read/write) to prevent hangs.
- Add `Authorization: Bearer <token>` via a single, centralized mechanism (interceptor).
- Handle `401` and `429` predictably (reauth, backoff).

Example with `dio`:

```dart
import 'package:dio/dio.dart';

class ApiClient {
  ApiClient({required this.baseUrl, required this.tokenStore}) {
    dio = Dio(BaseOptions(
      baseUrl: baseUrl,
      connectTimeout: const Duration(seconds: 10),
      receiveTimeout: const Duration(seconds: 20),
    ));

    dio.interceptors.add(InterceptorsWrapper(
      onRequest: (options, handler) async {
        final token = await tokenStore.readAccessToken();
        if (token != null && token.isNotEmpty) {
          options.headers['Authorization'] = 'Bearer $token';
        }
        handler.next(options);
      },
    ));
  }

  final String baseUrl;
  final TokenStore tokenStore;
  late final Dio dio;
}
```

### Certificate Pinning (Optional, Risk-Based)

Pinning can mitigate some MITM scenarios but increases operational risk (rollovers can break clients).

- If you implement pinning: pin public keys or SPKI hashes, plan rotation, and add telemetry for pin failures.
- Never disable TLS validation (`badCertificateCallback => true`) in production.

---

## Auth Flow Hardening

- Prefer short-lived access tokens + refresh tokens (server-side rotation).
- On `401`:
  - If refresh is supported: refresh once, retry once.
  - Otherwise: clear tokens and force login.
- Make "logout" clear local tokens immediately.

---

## Logging, Analytics, and Crash Reports

- Never log:
  - Authorization headers / tokens
  - Passwords / PINs / OTPs
  - GPS coordinates (unless explicitly required and consented)
  - Photo bytes / file paths with PII
- In release builds, reduce verbosity:

```dart
import 'package:flutter/foundation.dart';

void safeLog(String message) {
  if (!kReleaseMode) {
    // print is fine for debug; prefer a logger package if already used.
    // ignore: avoid_print
    print(message);
  }
}
```

If you use crash reporting, scrub PII from custom attributes and breadcrumbs.

---

## File Uploads (Photos) & PII

Client-side checks are UX and cost controls; the server must still enforce limits.

- Compress images before upload (bandwidth + storage).
- Enforce client-side max file size to reduce accidental uploads.
- Do not trust `Content-Type` or file extension; server must validate magic bytes.
- Avoid including PII in filenames; let the server assign names/paths.

---

## Permissions (Location/Camera)

- Request only what's needed, only when needed.
- Provide clear in-app explanations before the OS prompt (reduces social engineering risk).
- Handle denial gracefully; don't loop prompts.

---

## Offline Storage & Queueing

If you store queued requests (offline clock-in/out, pending uploads):

- Encrypt at rest if it contains tokens, GPS, or photos.
- Prefer storing **references** (IDs) rather than raw PII blobs.
- On logout, clear queued data unless explicitly intended to persist across accounts.

---

## UI/Screen Security (If Handling Sensitive Data)

- Consider blocking screenshots/screen recording on sensitive screens (Android `FLAG_SECURE`).
- Mask sensitive fields (passwords) and avoid autofill leaks if inappropriate.

---

## WebViews / Deep Links (If Used)

WebViews introduce browser-like risks:

- Disable JavaScript unless required.
- Block navigation to untrusted domains.
- Never pass tokens via URL query strings.

For deep links:
- Validate the link target and parameters before acting.
- Do not allow deep links to perform privileged actions without re-auth.

---

## Dependency Hygiene

- Keep packages updated; avoid abandoned libraries for auth, crypto, and networking.
- Prefer well-maintained packages; review permissions and platform code for critical deps.

---

## Minimal Security Test Checklist (Flutter)

- [ ] Token stored only in secure storage (not preferences)
- [ ] `Authorization` header added centrally; never logged
- [ ] `401` triggers safe logout/refresh behavior (no infinite retry loops)
- [ ] Photo uploads are size-bounded and compressed client-side
- [ ] Offline queue/caches cleared on logout (or encrypted, if retained)
