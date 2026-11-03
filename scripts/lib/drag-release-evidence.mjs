/** Evidence must come from ordered observer samples AFTER release, never another socket. */
export function releaseEvidence(log, releaseTime, releaseSeq, ids, endTime, eps, quietMs) {
  const baseline = new Map();
  const travel = new Map(ids.map((id) => [id, 0]));
  let seq = releaseSeq;
  let lastMotion = null;
  let frames = 0;
  for (const frame of log) {
    if (frame.t <= releaseTime || frame.seq === null || (seq !== null && frame.seq <= seq)) continue;
    seq = frame.seq;
    frames++;
    if (frame.moved && [...frame.moved.values()].some(d => d > eps)) lastMotion = frame.t;
    for (const id of ids) {
      const pos = frame.positions.get(id);
      if (!pos) continue;
      const prev = baseline.get(id);
      if (prev) {
        const d = Math.hypot(...pos.map((v, i) => v - prev[i]));
        travel.set(id, travel.get(id) + d);
        if (d > eps) lastMotion = frame.t;
      }
      baseline.set(id, pos);
    }
  }
  return {
    frames,
    travel: Object.fromEntries(travel),
    settleMs: lastMotion !== null && endTime - lastMotion >= quietMs ? lastMotion - releaseTime : null,
  };
}
