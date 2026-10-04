"use strict";
require("./natural_completion_native_bridge.cjs");
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path");
const { spawn } = require("node:child_process");
const test = require("node:test");
const root = process.env.POMODOROUGH_ROOT || path.resolve(__dirname, "../..");
const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const f = require(path.join(root, "server/web/test/p222-completion-fixture.js"));
const receipts = [];

async function server(t) {
  const directory = fs.mkdtempSync(path.join(temp, "core-pwa12-http-")), output = path.join(directory, "server.json");
  const overlay = path.join(directory, "overlay.json");
  fs.writeFileSync(overlay, JSON.stringify({ Replace: {
    [path.join(root, "server/internal/server/core_pwa12_http_test.go")]: path.join(__dirname, "natural_completion_http_test.go") } }));
  const child = spawn("go", ["test", "-overlay", overlay, "./internal/server", "-run", "^TestCorePWA12NaturalHTTP$", "-count=1", "-timeout=120s"],
    { cwd: path.join(root, "server"), env: { ...process.env, CORE_PWA12_HTTP_FIXTURE: output } });
  let logs = ""; child.stdout.on("data", (chunk) => { logs += chunk; }); child.stderr.on("data", (chunk) => { logs += chunk; });
  const stopped = new Promise((resolve, reject) => { child.on("error", reject); child.on("exit", resolve); });
  t.after(async () => { fs.writeFileSync(output + ".stop", "stop"); assert.equal(await stopped, 0, logs); });
  const deadline = Date.now() + 60000;
  while (!fs.existsSync(output)) {
    assert.equal(child.exitCode, null, logs); assert.ok(Date.now() < deadline, logs);
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  return JSON.parse(fs.readFileSync(output, "utf8"));
}

test("real Go natural response and native public Finish retain one history identity across exact retry", { timeout: 120000 }, async (t) => {
  const remote = await server(t), { client, core } = await f.fixture(t);
  const natural = JSON.parse(remote.naturalResponseRaw);
  assert.deepEqual(natural, remote.naturalResponse);
  t.mock.timers.setTime(remote.nowMs); client.use.trustedNow = () => Date.now();
  const user = { id: remote.userID, accountIncarnation: natural.accountIncarnation };
  Object.assign(client.state, { user, localOwnerId: f.sync.accountOwnerId(user), deviceId: remote.deviceID,
    csrfToken: remote.csrfToken, authenticated: true, sessionIdentityValidated: true });
  const start = { ...remote.startRequest.commands[0], deviceId: remote.deviceID };
  await f.seedMeta(client.use.database(), { snapshot: f.snapshot({ user, revision: 0, serverTime: start.occurredAt }), deviceId: remote.deviceID,
    deviceSequence: 1, uuidV7: null, hlc: { wallMs: start.hlcWallMs, counter: start.hlcCounter },
    canonicalHead: { wallMs: start.hlcWallMs, counter: start.hlcCounter },
    deliveryProof: { ...f.sync.emptyNeverSent(), commands: [start.id] },
    projectionPending: { ...f.sync.emptyNeverSent(), commands: [start] }, timerDependencies: [],
    workspaceObservation: { canonicalAnchorAt: null, commandTimes: {} },
    timerOwner: { timerId: natural.canonicalTimer.id, deviceId: remote.deviceID, tabId: client.use.tabId(), leaseExpiresAtMs: remote.nowMs + 60000 } });
  await f.seedQueues(client.use.database(), { commands: [start] });
  client.external.host.navigator.onLine = true;
  Object.assign(client.use, require(path.join(root, "server/web/app-sync.js")).create({ state: client.state, external: client.external, use: client.use, listen() {} }));
  Object.assign(client.use, require(path.join(root, "server/web/app-session.js")).create({ state: client.state, external: client.external, use: client.use, emit() {} }));
  await client.use.reloadPersistedState();
  const claim = await f.storage.claimWorkspaceBatch(client.use.database(), {
    ...client.use.captureAccountContext(), deviceId: remote.deviceID, localNowMs: remote.nowMs });
  const beforeInstall = await f.dump(client.use.database()), installationCalls = [], call = core.call.bind(core);
  core.call = (operation, input) => {
    const inputRaw = JSON.stringify(input), value = call(operation, input);
    installationCalls.push({ operation, inputRaw, input: JSON.parse(inputRaw), value: structuredClone(value) }); return value;
  };
  const sent = { ...f.sync.emptyNeverSent(), commands: remote.startRequest.commands };
  const installationReturn = await client.use.acceptSyncResponse(natural, sent, client.state.localOwnerId, null,
    client.use.captureAccountContext(), claim.claim);
  core.call = call;
  const installed = await f.dump(client.use.database());
  assert.deepEqual(f.meta(installed, "snapshot").canonicalTimer, natural.canonicalTimer);
  assert.deepEqual(f.meta(installed, "snapshot").history, natural.history);
  assert.deepEqual(installed.pending, []);
  assert.equal(f.meta(installed, "settings").selectedPhase, "short_break");
  const requests = []; let lose = true;
  client.external.host.fetch = async (url, input) => {
    const response = await fetch(remote.url + url, { ...input, headers: { ...input.headers, Authorization: `Bearer ${remote.accessToken}` } });
    const responseRaw = await response.clone().text(); assert.equal(response.status, 200, responseRaw);
    requests.push({ url, body: input.body, responseRaw });
    if (lose) { lose = false; throw new Error("Response lost after Go accepted natural Finish"); }
    return response;
  };
  await client.use.reloadPersistedState();
  const before = await f.dump(client.use.database()), model = client.use.getWorkspaceReadModel();
  assert.equal(model.display.phase, "short_break"); assert.ok(model.availableIntents.includes("finish"));
  assert.equal(await client.use.finishTimer(false), true, client.notices.join("; "));
  await client.use.syncNow(); const lost = await f.dump(client.use.database());
  client.use.database().close(); client.use.setDatabaseForTest(await client.use.openDatabase());
  await client.use.reloadPersistedState(); assert.deepEqual(await f.dump(client.use.database()), lost);
  await client.use.syncNow(); const after = await f.dump(client.use.database());
  assert.equal(requests[0].body, requests[1].body);
  const finished = JSON.parse(requests[0].responseRaw);
  assert.equal(finished.acknowledgements[0].outcome, "applied");
  assert.equal(finished.history.length, 1); assert.equal(finished.history[0].id, natural.history[0].id);
  assert.equal(finished.history[0].commandId, JSON.parse(requests[0].body).commands[0].id);
  assert.deepEqual(f.meta(after, "snapshot").canonicalTimer, finished.canonicalTimer);
  assert.deepEqual(f.meta(after, "snapshot").history, finished.history);
  assert.equal(f.meta(after, "settings").selectedPhase, "short_break");
  assert.deepEqual(after.pending, []); assert.equal(client.use.getWorkspaceReadModel().cadence.completedFocusTotal, 1);
  assert.equal(core.call("core.version", {}).coreVersion, "0.45.0");
  receipts.push({ naturalResponseRaw: remote.naturalResponseRaw, startRequest: remote.startRequest,
    beforeInstall, installed, installationReturn: installationReturn ?? null, installationCalls, before, model, requests, lost, after });
});

test.after(() => fs.writeFileSync(path.join(temp, "core-pwa12-http-green.json"), JSON.stringify({ receipts }, null, 2)));
