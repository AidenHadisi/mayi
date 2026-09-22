# Security Policy

mayi sits in the permission path of AI coding agents. A bug that lets a blocked action through is a security bug.

## Reporting a vulnerability

Do not open a public issue. Email **so.hadisi@gmail.com** with a description, steps to reproduce, and the affected version. You will get an acknowledgement within 72 hours, and a fix or mitigation plan before any public disclosure.

## Guarantees

- **Fail closed.** Network errors, timeouts, malformed responses, and panics deny the action. They never allow it.
- **No execution.** The hook never runs the command it is judging.
- **rustls.** TLS uses rustls and the platform verifier. `cargo deny` rejects OpenSSL.
- **Local logs.** Decision logs stay on the machine. Only the action line is sent to the model API.

## Supported versions

Only the latest release receives security fixes.
