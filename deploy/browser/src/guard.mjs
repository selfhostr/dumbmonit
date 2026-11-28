// Filtre SSRF : adresses refusées (deny_cidrs) et résolution DNS.
import { BlockList, isIP } from 'node:net';
import dns from 'node:dns/promises';

// Normalise une adresse : retire les crochets IPv6 et la zone, remplace ::ffff:a.b.c.d par a.b.c.d.
export function normalizeIp(addr) {
  let a = String(addr).trim();
  if (a.startsWith('[') && a.endsWith(']')) a = a.slice(1, -1);
  const zone = a.indexOf('%');
  if (zone >= 0) a = a.slice(0, zone);
  const m = /^::ffff:(\d+\.\d+\.\d+\.\d+)$/i.exec(a);
  if (m) return m[1];
  // Forme hexadécimale ::ffff:7f00:1
  const h = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/i.exec(a);
  if (h) {
    const hi = parseInt(h[1], 16);
    const lo = parseInt(h[2], 16);
    return `${hi >> 8}.${hi & 255}.${lo >> 8}.${lo & 255}`;
  }
  return a;
}

// Construit la liste de blocage ; lève une Error sur un CIDR invalide.
export function buildDenyList(cidrs) {
  const list = new BlockList();
  for (const c of cidrs) {
    const [addr, bits, extra] = String(c).split('/');
    const fam = isIP(addr);
    const max = fam === 4 ? 32 : 128;
    const prefix = bits === undefined ? max : Number(bits);
    if (!fam || extra !== undefined || !Number.isInteger(prefix) || prefix < 0 || prefix > max || bits === '') {
      throw new Error(`invalid CIDR '${String(c).slice(0, 64)}'`);
    }
    list.addSubnet(addr, prefix, fam === 4 ? 'ipv4' : 'ipv6');
  }
  return list;
}

export function isDenied(list, addr) {
  const ip = normalizeIp(addr);
  const fam = isIP(ip);
  if (!fam) return true; // ce n'est pas une adresse : par prudence, refusé
  return list.check(ip, fam === 4 ? 'ipv4' : 'ipv6');
}

export function hostOf(url) {
  return new URL(url).hostname.toLowerCase().replace(/^\[|\]$/g, '');
}

// Résout un hôte (littéral IP ou nom). Renvoie la liste des adresses, [] si échec.
export async function resolveHost(host, lookup = dns.lookup) {
  const h = host.replace(/^\[|\]$/g, '');
  if (isIP(normalizeIp(h))) return [normalizeIp(h)];
  try {
    const res = await lookup(h, { all: true, verbatim: true });
    return res.map((r) => normalizeIp(r.address));
  } catch {
    return [];
  }
}

// Verdict pour un hôte : { ok, addrs, reason }. Toutes les adresses doivent être autorisées.
export async function checkHost(host, { denyList, allowPrivate, allowHosts }, lookup) {
  const h = host.toLowerCase().replace(/^\[|\]$/g, '');
  if (allowPrivate || allowHosts.has(h)) return { ok: true, addrs: [], reason: null };
  const addrs = await resolveHost(h, lookup);
  if (addrs.length === 0) return { ok: true, addrs, reason: 'unresolved' }; // le navigateur échouera seul
  if (addrs.some((a) => isDenied(denyList, a))) return { ok: false, addrs, reason: 'deny_cidr' };
  return { ok: true, addrs, reason: null };
}

// Règle --host-resolver-rules : fige les hôtes des goto déjà validés (anti DNS rebinding).
export function hostResolverRules(mappings) {
  const rules = [];
  for (const [host, addr] of mappings) {
    if (!/^[a-z0-9.-]+$/.test(host)) continue; // jamais d'injection dans la ligne de commande
    rules.push(`MAP ${host} ${isIP(addr) === 6 ? `[${addr}]` : addr}`);
  }
  return rules.join(', ');
}
