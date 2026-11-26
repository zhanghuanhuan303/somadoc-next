# Security policy

The project lives in [`soma-typeset/`](soma-typeset/). The full policy is
[`soma-typeset/SECURITY.md`](soma-typeset/SECURITY.md).

Please report vulnerabilities through GitHub's private security advisory form:

<https://github.com/zhanghuanhuan303/somadoc-next/security/advisories/new>

Do not open a public issue for a vulnerability. The public Studio is a static
browser-local bundle with no `/api` (ADR-0121). The development host binds
loopback by default and has no authentication; do not expose port 7070
publicly.
