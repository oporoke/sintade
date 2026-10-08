// Day 87: 200 concurrent viewers on the playback path (docs/design.md §18: k6).
//
// Each virtual viewer behaves like a person opening a share link: watch data, the playback
// grant, the HLS master, a rung with the token the master handed out, the init segment and a
// media segment from the bucket (signed URL), and the scrub sprite's cue file. Every viewer has
// its own address in X-Forwarded-For, as real viewers behind the proxy would, so the per-IP
// watch limit (120/min) is in force and not worked around.
//
// Run through `just load-playback` (k6 from its Docker image; needs `just load-prepare` first).
import http from 'k6/http';
import { check, sleep } from 'k6';

const API = __ENV.API_URL || 'http://localhost:8080';
const SLUG = __ENV.SLUG;
// Signed bucket URLs name the public origin (in dev the Angular proxy on :4200, which forwards to
// MinIO). The signature covers the Host header, so the script asks MinIO itself and keeps the
// original Host: the proxy hop is skipped, the bucket is what is measured.
const BUCKET = __ENV.BUCKET_URL || 'http://localhost:9010';
const VUS = Number(__ENV.VUS || 200);

export const options = {
  scenarios: {
    viewers: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '15s', target: VUS },
        { duration: __ENV.HOLD || '45s', target: VUS },
        { duration: '5s', target: 0 },
      ],
      gracefulRampDown: '10s',
    },
  },
  thresholds: {
    // "200 concurrent viewers without errors": nothing fails, no check fails.
    http_req_failed: ['rate==0'],
    checks: ['rate==1'],
    // docs/design.md §11: API p95 <= 100 ms.
    'http_req_duration{kind:api}': ['p(95)<100'],
  },
};

export function setup() {
  if (!SLUG) {
    throw new Error('SLUG is not set: run `just load-prepare` first');
  }
}

function ipFor(vu) {
  // 198.18.0.0/15 is reserved for benchmarking (RFC 2544).
  return `198.18.${Math.floor(vu / 250)}.${(vu % 250) + 1}`;
}

let logged = 0;
function seen(name, response) {
  // The first few surprises, so a failing run says why (status and problem title).
  if (response.status !== 200 && logged < 5) {
    logged += 1;
    console.warn(`${name}: ${response.status} ${String(response.body).slice(0, 160)}`);
  }
}

export default function () {
  try {
    viewer();
  } finally {
    // A viewer watching: well under the 120/min per-address limit. Also after a failure, so a
    // broken run is not a flood.
    sleep(4 + Math.random() * 2);
  }
}

function viewer() {
  const headers = { 'X-Forwarded-For': ipFor(__VU) };
  const api = { headers, tags: { kind: 'api' } };
  const bucket = { tags: { kind: 'bucket' } };

  const watch = http.get(`${API}/api/v1/s/${SLUG}`, api);
  seen('watch data', watch);
  check(watch, { 'watch data 200': (r) => r.status === 200 });

  const playback = http.get(`${API}/api/v1/s/${SLUG}/playback`, api);
  seen('playback', playback);
  const playbackOk = check(playback, { 'playback 200': (r) => r.status === 200 });
  if (!playbackOk) {
    return;
  }
  const grant = playback.json();
  const offered = check(grant, {
    'grant offers the ladder': (g) => !!g.hls_url,
    'grant offers the sprite': (g) => !!g.sprite_url,
  });
  if (!offered) {
    return;
  }

  const master = http.get(`${API}${grant.hls_url}`, api);
  seen('master', master);
  if (!check(master, { 'master 200': (r) => r.status === 200 })) {
    return;
  }
  const rungLine = master.body.split('\n').find((line) => line.includes('/index.m3u8?t='));
  if (!check(rungLine, { 'master names a rung with a token': (l) => !!l })) {
    return;
  }
  // The master names rungs relative to itself (`720p/index.m3u8?t=...`).
  const rungUrl = `${API}${grant.hls_url.replace(/[^/]*$/, '')}${rungLine.trim()}`;

  const rung = http.get(rungUrl, api);
  seen('rung', rung);
  if (!check(rung, { 'rung 200': (r) => r.status === 200 })) {
    return;
  }
  const uris = rung.body
    .split('\n')
    .map((line) => line.match(/URI="([^"]+)"/)?.[1] || (line.startsWith('http') ? line : null))
    .filter(Boolean);
  check(uris, { 'rung has signed init and segments': (u) => u.length >= 2 });
  for (const uri of uris.slice(0, 2)) {
    const [, host, path] = uri.match(/^https?:\/\/([^/]+)(\/.*)$/);
    const part = http.get(`${BUCKET}${path}`, { headers: { Host: host }, tags: bucket.tags });
    seen('segment', part);
    check(part, { 'init/segment 200': (r) => r.status === 200 });
  }

  const sprite = http.get(`${API}${grant.sprite_url}`, api);
  seen('sprite', sprite);
  check(sprite, { 'sprite cues 200': (r) => r.status === 200 });
}
