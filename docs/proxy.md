# Proxies

driller sends all HTTP traffic through [reqwest], which honors the standard
proxy environment variables. **No flag or benchmark-file setting is required** —
driller picks them up automatically.

## Environment variables

| Variable | Effect |
|---|---|
| `HTTP_PROXY` / `http_proxy` | Proxy for `http://` requests |
| `HTTPS_PROXY` / `https_proxy` | Proxy for `https://` requests |
| `ALL_PROXY` / `all_proxy` | Proxy for both schemes (less specific than the two above) |
| `NO_PROXY` / `no_proxy` | Comma-separated hosts/domains/CIDRs that bypass the proxy |

If both `ALL_PROXY` and a scheme-specific variable are set, the more specific
`HTTP_PROXY` / `HTTPS_PROXY` wins.

## Enterprise example: proxy out, internal direct

The common corporate-network case is to route external traffic through the
proxy while reaching internal targets directly. That is exactly what `NO_PROXY`
is for:

```bash
# Send external load through the corporate proxy...
export HTTPS_PROXY=http://proxy.corp.example:8080
# ...but connect to internal targets directly
export NO_PROXY=.internal,localhost,127.0.0.1,10.0.0.0/8

driller run https://api.partner.example/health --stats
```

`NO_PROXY` accepts:

- exact hostnames (`localhost`),
- dot-prefixed domain suffixes that match any subdomain (`.internal` matches
  `svc.internal`),
- IP addresses and CIDR ranges (`10.0.0.0/8`).

Targets that match `NO_PROXY` connect directly — what you want when
benchmarking internal services from inside the corporate network.

## Proxy authentication

Credentials can be supplied in the proxy URL's userinfo:

```bash
export HTTPS_PROXY=http://user:password@proxy.corp.example:8080
```

## SOCKS proxies

SOCKS proxies (`socks5://…`) are **not** supported by the published driller
binary: they require reqwest's `socks` feature, which driller does not enable.
Use an HTTP/HTTPS proxy, or build driller yourself with that feature turned on.

[reqwest]: https://docs.rs/reqwest
