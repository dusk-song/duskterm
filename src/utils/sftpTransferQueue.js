// Bound transfers across batches and panel instances without hiding newly added tasks.
const lanes = new Map();
const UPLOAD_CONCURRENCY = 3;

function drain(key, lane) {
  while (lane.active < lane.limit) {
    const index = lane.pending.findIndex(({ task, destination }) =>
      task.status === 'cancelled' || task.status === 'skipped'
      || !destination || !lane.destinations.has(destination));
    if (index < 0) break;
    const { task, run, destination, resolve, reject } = lane.pending.splice(index, 1)[0];
    if (task.status === 'cancelled' || task.status === 'skipped') {
      resolve();
      continue;
    }
    lane.active += 1;
    if (destination) lane.destinations.add(destination);
    Promise.resolve().then(() => {
      if (task.status === 'cancelled' || task.status === 'skipped') return;
      return run(task);
    }).then(resolve, reject).finally(() => {
      lane.active -= 1;
      if (destination) lane.destinations.delete(destination);
      drain(key, lane);
    });
  }
  if (!lane.active && !lane.pending.length && lanes.get(key) === lane) lanes.delete(key);
}

export function enqueueSftpTransfer(task, run) {
  const key = JSON.stringify([task.sessionId, task.direction]);
  let lane = lanes.get(key);
  if (!lane) {
    lane = { limit: task.direction === 'upload' ? UPLOAD_CONCURRENCY : 1, active: 0, pending: [], destinations: new Set() };
    lanes.set(key, lane);
  }
  return new Promise((resolve, reject) => {
    // Reserve the destination through preflight, transfer and cleanup. A later upload
    // to the same path must check existence only after its predecessor has finished.
    const destination = task.direction === 'upload' ? task.remotePath : task.localPath;
    lane.pending.push({ task, run, destination, resolve, reject });
    drain(key, lane);
  });
}
