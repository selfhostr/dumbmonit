import test from 'node:test';
import assert from 'node:assert/strict';
import { buildDenyList, checkHost, hostResolverRules, isDenied, normalizeIp } from '../src/guard.mjs';

const DENY = ['127.0.0.0/8', '169.254.0.0/16', '::1/128', 'fe80::/10', 'fd00:ec2::254/128', '0.0.0.0/8', '10.0.0.0/8'];
const list = buildDenyList(DENY);

test('adresses refusées et autorisées', () => {
  for (const a of ['127.0.0.1', '169.254.169.254', '::1', 'fe80::1', 'fd00:ec2::254', '0.0.0.0', '10.1.2.3', '::ffff:127.0.0.1', '::ffff:7f00:1', '[::1]']) {
    assert.equal(isDenied(list, a), true, a);
  }
  for (const a of ['93.184.216.34', '2606:2800:220:1:248:1893:25c8:1946', '::ffff:8.8.8.8']) {
    assert.equal(isDenied(list, a), false, a);
  }
  assert.equal(isDenied(list, 'not-an-ip'), true);
});

test('normalizeIp', () => {
  assert.equal(normalizeIp('::ffff:1.2.3.4'), '1.2.3.4');
  assert.equal(normalizeIp('fe80::1%eth0'), 'fe80::1');
});

test('CIDR invalides refusés', () => {
  for (const c of ['nope', '10.0.0.0/33', '::1/129', '10.0.0.0/', '1.2.3.4/8/9', '10.0.0.0/-1']) {
    assert.throws(() => buildDenyList([c]), /invalid CIDR/, c);
  }
});

const guard = (o = {}) => ({ denyList: list, allowPrivate: false, allowHosts: new Set(), ...o });
const fake = (addrs) => async () => addrs.map((address) => ({ address }));

test('checkHost : DNS, littéraux, allow_private et allow_hosts', async () => {
  assert.equal((await checkHost('a.test', guard(), fake(['93.184.216.34']))).ok, true);
  assert.equal((await checkHost('a.test', guard(), fake(['93.184.216.34', '10.0.0.5']))).ok, false);
  assert.equal((await checkHost('a.test', guard(), fake(['127.0.0.1']))).reason, 'deny_cidr');
  assert.equal((await checkHost('127.0.0.1', guard(), fake([]))).ok, false);
  assert.equal((await checkHost('[::1]', guard(), fake([]))).ok, false);
  assert.equal((await checkHost('127.0.0.1', guard({ allowPrivate: true }), fake([]))).ok, true);
  assert.equal((await checkHost('internal.test', guard({ allowHosts: new Set(['internal.test']) }), fake(['10.0.0.1']))).ok, true);
  const failing = async () => {
    throw new Error('ENOTFOUND');
  };
  assert.equal((await checkHost('gone.test', guard(), failing)).reason, 'unresolved');
});

test('hostResolverRules', () => {
  assert.equal(
    hostResolverRules(new Map([['a.test', '1.2.3.4'], ['b.test', '2001:db8::1']])),
    'MAP a.test 1.2.3.4, MAP b.test [2001:db8::1]',
  );
  assert.equal(hostResolverRules(new Map([['a b,c', '1.2.3.4']])), '');
});
