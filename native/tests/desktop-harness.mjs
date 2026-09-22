// Run inside a disposable desktop. It exercises the real built MCP executable;
// no mock UI or transport. commands.json is a test driver, never a product API.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';

const root = resolve(process.argv[2] ?? 'native/.test-data/desktop');
await mkdir(root, { recursive: true });
const executable = process.env.AGENTUI_TEST_EXE ?? resolve(import.meta.dirname, '../AgentUI/bin/Release/net48/AgentUI.exe');
const clients = [];
function client(name) {
  const child = spawn(executable, ['--stdio', '--no-delivery', '--data-dir', root, '--agent', name], { windowsHide: true });
  let sequence = 0;
  const pending = new Map();
  createInterface({ input: child.stdout }).on('line', line => {
    const message = JSON.parse(line);
    if (message.id !== undefined) {
      const waiter = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) waiter.reject(new Error(JSON.stringify(message.error))); else waiter.resolve(message.result);
    }
  });
  child.stderr.on('data', chunk => console.log(name, String(chunk).trim()));
  const send = (method, params) => new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
  const result = { child, send, call: (name, args) => send('tools/call', { name, arguments: args }) };
  clients.push(result);
  return result;
}
async function ready(client) {
  await client.send('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'Desktop verification', version: '1' } });
  client.child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
}
const first = client('Codex · test'); await ready(first);
const result = await first.call('feedback_request', {
  question: 'How should AgentUI announce a new question?',
  context: 'Choose the default for this desktop prototype. Questions remain in your inbox while your agent keeps working.',
  project: 'AgentUI', options: ['Show the panel quietly', 'Notify, then open on click']
});
const id = result.structuredContent.requestId;
const second = client('Second agent · test'); await ready(second);
await writeFile(join(root, 'harness.json'), JSON.stringify({ firstPid: first.child.pid, secondPid: second.child.pid, requestId: id }));
console.log('READY', JSON.stringify({ firstPid: first.child.pid, secondPid: second.child.pid, requestId: id }));
let lastCommand = '', lastStatus = '';
const timer = setInterval(async () => {
  try {
    const command = await readFile(join(root, 'commands.json'), 'utf8').catch(() => '');
    if (command && command !== lastCommand) {
      lastCommand = command;
      const { action } = JSON.parse(command);
      if (action === 'another') {
        const result = await second.call('feedback_request', {
          question: 'How long should a postponed question stay quiet?', project: 'AgentUI',
          context: 'A new question from a second agent should leave the note you are writing intact.',
          fields: [{ id: 'minutes', label: 'Minutes', type: 'number', required: true, min: 1, max: 60 },
                   { id: 'sound', label: 'Play a sound', type: 'boolean' }]
        });
        console.log('ADDED', result.content[0].text);
      }
      if (action === 'kill-first') { first.child.kill(); console.log('OWNER TERMINATED'); }
      if (action === 'stop') { clearInterval(timer); for (const c of clients) if (!c.child.killed) c.child.stdin.end(); return; }
    }
    const state = await second.call('feedback_read', { requestIds: [id] });
    const serialized = JSON.stringify(state.structuredContent);
    if (serialized !== lastStatus) {
      lastStatus = serialized;
      await writeFile(join(root, 'observed.json'), serialized);
      console.log('AGENT RECEIVED', serialized);
    }
  } catch (error) { console.log('Harness:', error.message); }
}, 500);
process.on('SIGTERM', () => { for (const c of clients) c.child.kill(); process.exit(); });
