# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in driller, please report it responsibly.

**Email:** zoosky@gmail.com

Please include:

- A description of the vulnerability
- Steps to reproduce
- The version of driller affected
- Any potential impact

You should receive a response within 48 hours. We will work with you to understand and address the issue before any public disclosure.

## Scope

Driller is a load-testing tool designed to run against systems you own or have permission to test. Security issues in driller itself (e.g., command injection via YAML configs, credential leaks in output, dependency vulnerabilities) are in scope.

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.10.x  | Yes       |
| < 0.10  | No (upstream `drill`, unmaintained) |
