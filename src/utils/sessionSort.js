const hostCollator = new Intl.Collator('en', { numeric: true, sensitivity: 'base' });

function getHostSortKey(value) {
  const host = String(value || '').trim();
  if (!host) return { kind: 3, host };

  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host)) {
    const parts = host.split('.').map(Number);
    if (parts.every(part => part <= 255)) return { kind: 0, parts };
  }

  const address = host.startsWith('[') && host.endsWith(']') ? host.slice(1, -1) : host;
  if (address.includes(':') && /^[\da-f:.]+$/i.test(address)) {
    try {
      // URL validates and canonicalizes IPv6, including embedded IPv4, without DNS.
      const canonical = new URL(`http://[${address}]/`).hostname.slice(1, -1);
      const [left, right] = canonical.split('::');
      const start = left ? left.split(':') : [];
      const end = right ? right.split(':') : [];
      const parts = right === undefined
        ? start
        : [...start, ...Array(8 - start.length - end.length).fill('0'), ...end];
      return { kind: 1, parts: parts.map(part => parseInt(part, 16)) };
    } catch {
      // Invalid addresses remain visible alongside hostnames; connection validation is unchanged.
    }
  }

  return { kind: 2, host };
}

export function sortSessionsByHost(sessions) {
  // Parse once per session and keep the store array and session objects untouched.
  return sessions.map(session => ({ session, key: getHostSortKey(session.host) }))
    .sort((a, b) => {
      const kindOrder = a.key.kind - b.key.kind;
      if (kindOrder) return kindOrder;
      if (a.key.parts) {
        for (let index = 0; index < a.key.parts.length; index++) {
          const difference = a.key.parts[index] - b.key.parts[index];
          if (difference) return difference;
        }
        return 0;
      }
      return hostCollator.compare(a.key.host, b.key.host);
    })
    .map(({ session }) => session);
}
