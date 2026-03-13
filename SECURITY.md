# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in git-valet, please report it responsibly by opening a private security advisory on GitHub.

Do not open a public issue for security vulnerabilities.

## Scope

git-valet manages file paths and git operations. Security concerns include:
- Path traversal in tracked file paths
- Unintended file exposure through misconfigured valet repos
- Hook injection via crafted config files
