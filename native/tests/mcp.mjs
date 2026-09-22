import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { argv, env } from 'node:process';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import assert from 'node:assert/strict';

const executable = env.AGENTUI_TEST_EXE ?? resolve(import.meta.dirname, '../AgentUI/bin/Release/net48/AgentUI.exe');
const metadataMode = argv.includes('--codex-metadata');
const root = await mkdtemp(join(tmpdir(), 'agentui-mcp-'));
const pending = new Map();
const process = spawn(executable, ['--stdio', '--headless', metadataMode ? '--codex' : '--no-delivery', '--data-dir', root], {
  windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
  env: { ...env, CODEX_THREAD_ID: '', AGENTUI_CODEX_EXE: resolve(import.meta.dirname, '../AgentUI.Tests/bin/Release/net48/AgentUI.Tests.exe') }
});
let seq = 0, stderr = '';
process.stderr.on('data', chunk => stderr += chunk);
const lines = createInterface({ input: process.stdout });
lines.on('line', line => {
  let message;
  try { message = JSON.parse(line); } catch { throw new Error(`Non-protocol stdout: ${line}`); }
  if (message.id !== undefined) {
    const waiter = pending.get(message.id);
    assert.ok(waiter, 'response has a matching request');
    pending.delete(message.id);
    clearTimeout(waiter.timer);
    message.error ? waiter.reject(new Error(JSON.stringify(message.error))) : waiter.resolve(message.result);
  }
});
function send(method, params) {
  const id = ++seq;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`Timed out: ${method}\n${stderr}`)), 10000);
    pending.set(id, { resolve, reject, timer });
    process.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
}
async function call(name, args) {
  const result = await send('tools/call', { name, arguments: args });
  assert.deepEqual(JSON.parse(result.content[0].text), result.structuredContent, 'text and structured output agree');
  return result;
}
try {
  const initialized = await send('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'AgentUI test', version: '1' } });
  assert.equal(initialized.serverInfo.name, 'AgentUI');
  process.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  const tools = await send('tools/list', {});
  assert.equal(tools.tools.length, 3);
  assert.ok(Buffer.byteLength(JSON.stringify(tools)) < 8192, 'tool catalog fits 8 KiB budget');
  const started = performance.now();
  const first = (await call('feedback_request', { question: 'Which path?', options: ['A', 'B'] })).structuredContent;
  assert.ok(performance.now() - started < 1000, 'request returns without waiting for a human');
  const second = (await call('feedback_request', { question: 'A separate question' })).structuredContent;
  assert.notEqual(first.requestId, second.requestId);
  const before = (await call('feedback_read', { requestIds: [first.requestId, second.requestId] })).structuredContent;
  assert.ok(before.requests.every(x => x.status === 'pending'));
  const waiting = call('feedback_read', { requestIds: [first.requestId], waitMs: 3000 });
  await call('feedback_cancel', { requestId: first.requestId, reason: 'Test is complete' });
  assert.equal((await waiting).structuredContent.requests[0].status, 'cancelled', 'an outstanding read wakes on cancellation');
  assert.equal((await call('feedback_read', { requestIds: [first.requestId] })).structuredContent.requests[0].status, 'cancelled', 'reads do not consume answers');
  assert.equal((await call('feedback_read', { requestIds: [second.requestId], waitMs: 30 })).structuredContent.timedOut, true);
  assert.equal((await call('feedback_request', { question: 'Invalid', unknown: true })).isError, true);
  assert.equal((await call('feedback_request', { question: 'Invalid', fields: [{ id: 'x', label: 'X', type: 'script' }] })).isError, true);
  assert.equal((await call('feedback_read', { requestIds: ['../outside'] })).isError, true);
  assert.equal((await call('feedback_read', { requestIds: [second.requestId], waitMs: -1 })).isError, true);
  assert.equal((await call('feedback_read', { requestIds: [second.requestId], waitMs: 999999999999 })).isError, true);
  if (metadataMode) {
    assert.equal(first.automaticDelivery, false, 'missing task metadata does not promise delivery');
    for (const threadId of ['11111111-1111-4111-8111-111111111111', '22222222-2222-4222-8222-222222222222']) {
      const result = await send('tools/call', { name: 'feedback_request', arguments: { question: 'Metadata routing check' }, _meta: { threadId } });
      assert.equal(result.structuredContent.automaticDelivery, true);
      const stored = JSON.parse(await readFile(join(root, 'requests', result.structuredContent.requestId + '.json'), 'utf8'));
      assert.equal(stored.threadId, threadId, 'each request follows its own task metadata');
    }
    assert.equal((await send('tools/call', { name: 'feedback_request', arguments: { question: 'Bad metadata' }, _meta: { threadId: 'not-a-task' } })).isError, true);
  }
  assert.equal((await call('feedback_request', { question: 'Still alive after invalid input' })).isError, false);
  console.log(`PASS stdio integration${metadataMode ? ' with Codex routing' : ''}: catalog ${Buffer.byteLength(JSON.stringify(tools))} bytes, creation/reads/wait/cancel/validation/output agreement`);
} finally {
  process.stdin.end();
  await Promise.race([new Promise(resolve => process.once('exit', resolve)), new Promise((_, reject) => setTimeout(() => { process.kill(); reject(new Error('Server did not stop on EOF')); }, 4000).unref())]);
  for (const waiter of pending.values()) clearTimeout(waiter.timer);
  lines.close();
  await rm(root, { recursive: true, force: true });
}
