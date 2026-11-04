'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, 'browser-fixture.html'), 'utf8');
const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)];
assert.equal(scripts.length, 1);
const script = scripts[0][1];
const boundary = script.indexOf('\nconst field = document.getElementById');
assert.ok(boundary > 0, 'pure-oracle boundary missing');
const {PREFIX, SUFFIX, SCENARIOS, advance, evaluate, newRun} = vm.runInNewContext(
  script.slice(0, boundary) + '\n({PREFIX, SUFFIX, SCENARIOS, advance, evaluate, newRun})');

assert.equal(PREFIX.length, [...PREFIX].length + 1);
assert.equal(Array.from(SCENARIOS, s => s.id).sort().join('|'), 'auto|auto-ru|manual|reverse');

for (const scenario of SCENARIOS) {
  const run = newRun();
  run.counts.compositionstart = 1;
  run.counts.compositionend = 1;
  for (const checkpoint of scenario.checkpoints) {
    const value = PREFIX + checkpoint + SUFFIX;
    const caret = PREFIX.length + checkpoint.length;
    run.reached = advance(scenario, run.reached, value, caret, caret);
  }
  const final = scenario.checkpoints.at(-1);
  const value = PREFIX + final + SUFFIX;
  const caret = PREFIX.length + final.length;
  assert.ok(final.endsWith('\n'));
  const result = evaluate(scenario, run, value, caret, caret);
  assert.equal(result.pass, true);

  // Equal lengths and a valid caret cannot hide damage elsewhere in the field
  assert.equal(evaluate(scenario, run, 'X' + value.slice(1), caret, caret).pass, false);
  assert.equal(evaluate(scenario, run, value.slice(0, -1) + '!', caret, caret).pass, false);
  assert.equal(evaluate(scenario, run, value, caret - 1, caret - 1).pass, false);
  assert.equal(evaluate(scenario, run, value, caret - 1, caret).pass, false);
  assert.equal(evaluate(scenario, {...run, reached: 0}, value, caret, caret).pass, false);
  assert.equal(evaluate(scenario, {...run, composing: 1}, value, caret, caret).pass, false);
  assert.equal(evaluate(scenario, {...run, untrusted: 1}, value, caret, caret).pass, false);
  assert.equal(evaluate(scenario, {...run, counts: newRun().counts}, value, caret, caret).pass,
               false);
  assert.equal(evaluate(scenario, {
    ...run, counts: {...run.counts, compositionend: 2},
  }, value, caret, caret).pass, false);

  const skipped = newRun();
  skipped.counts = run.counts;
  skipped.reached = advance(scenario, 0, value, caret, caret);
  assert.equal(evaluate(scenario, skipped, value, caret, caret).pass, false);
  const report = JSON.stringify(result);
  for (const checkpoint of scenario.checkpoints)
    assert.equal(report.includes(checkpoint.trim()), false, 'report contains field text');
}

const observerBoundary = script.indexOf('// The only programmatic edit or caret placement;');
assert.ok(observerBoundary > boundary, 'observer boundary missing');
for (const scenario of SCENARIOS) {
  const listeners = {};
  const field = {
    value: PREFIX + SUFFIX, selectionStart: PREFIX.length, selectionEnd: PREFIX.length,
    addEventListener(type, listener) { listeners[type] = listener; },
  };
  const nodes = {field, scenario: {}, summary: {}, report: {}};
  const document = {
    getElementById(id) { return nodes[id]; },
    addEventListener(type, listener) { listeners[type] = listener; },
  };
  const observed = vm.runInNewContext(script.slice(0, observerBoundary) +
    '\n({setScenario(id) {scenario = SCENARIOS.find(s => s.id === id); run = newRun();}})',
    {document});
  observed.setScenario(scenario.id);
  const event = (type, inputType) => listeners[type]({type, inputType, isTrusted: true});
  const report = () => JSON.parse(nodes.report.textContent);
  event('selectionchange');
  assert.equal(report().checkpoints_reached, '0/' + scenario.checkpoints.length);
  assert.equal(report().counts.checkpoint_advances_selectionchange, 0,
               'reset selectionchange must not advance');
  event('compositionstart');
  let reached = 0;
  for (const checkpoint of scenario.checkpoints) {
    const caret = PREFIX.length + checkpoint.length;
    const committedLength = scenario.id === 'auto' && reached > 0 ?
      scenario.checkpoints[reached - 1].length :
      scenario.id === 'auto-ru' && reached > 1 ? scenario.checkpoints[reached - 1].length : 0;
    field.value = PREFIX + checkpoint + SUFFIX;
    field.selectionStart = PREFIX.length + committedLength;
    field.selectionEnd = caret;
    event('input', 'insertCompositionText');
    assert.equal(report().checkpoints_reached, reached + '/' + scenario.checkpoints.length);
    field.selectionStart = caret;
    event('selectionchange');
    reached++;
    assert.equal(report().checkpoints_reached, reached + '/' + scenario.checkpoints.length);
  }
  event('compositionend');
  assert.equal(report().pass, true, 'late caret must complete exact checkpoints');
  assert.equal(report().counts.checkpoint_advances_selectionchange, reached);
  assert.equal(report().counts.checkpoint_advances_input, 0);
  assert.equal(report().counts.checkpoint_advances_composition, 0);

  field.selectionStart--;
  event('selectionchange');
  assert.equal(report().checks.caret, false);
  assert.equal(report().pass, false, 'later selection must not leave a stale pass');

  observed.setScenario(scenario.id);
  field.value = PREFIX + SUFFIX;
  field.selectionStart = field.selectionEnd = PREFIX.length;
  event('selectionchange');
  event('compositionstart');
  for (const checkpoint of scenario.checkpoints) {
    field.value = PREFIX + checkpoint + SUFFIX;
    field.selectionStart = field.selectionEnd = PREFIX.length + checkpoint.length;
    event('input', 'insertCompositionText');
    event('selectionchange');
  }
  event('compositionend');
  assert.equal(report().pass, true);
  assert.equal(report().counts.checkpoint_advances_input, scenario.checkpoints.length);
  assert.equal(report().counts.checkpoint_advances_selectionchange, 0,
               'selectionchange must not double-count input checkpoints');
  assert.equal(report().counts.checkpoint_advances_composition, 0);

  // Reaching the final field by selection changes cannot replace missing earlier checkpoints
  observed.setScenario(scenario.id);
  field.selectionStart = field.selectionEnd;
  event('compositionstart');
  event('input', 'insertCompositionText');
  event('compositionend');
  event('selectionchange');
  assert.equal(report().checks.field, true);
  assert.equal(report().checks.caret, true);
  assert.equal(report().checks.checkpoints, false);
  assert.equal(report().pass, false);

  observed.setScenario(scenario.id);
  field.value = PREFIX + scenario.checkpoints[0] + SUFFIX;
  field.selectionStart = PREFIX.length;
  field.selectionEnd = PREFIX.length + scenario.checkpoints[0].length;
  event('input', 'insertCompositionText');
  field.value = PREFIX + scenario.checkpoints[1] + SUFFIX;
  field.selectionStart = field.selectionEnd = PREFIX.length + scenario.checkpoints[1].length;
  event('input', 'insertCompositionText');
  event('selectionchange');
  assert.equal(report().checkpoints_reached, '0/' + scenario.checkpoints.length,
               'a late caret must not recover a checkpoint whose text was overwritten');
}

console.log('browser fixture oracle: 4 scenarios and late-caret observers passed; ' +
            'JavaScript only, no browser or IMK input');
