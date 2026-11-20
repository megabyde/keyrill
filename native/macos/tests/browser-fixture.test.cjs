'use strict';
// Checks the browser fixture's oracle in Node, without a browser or an input method. The page's
// one inline script is cut at two markers: the part before `const field` is the pure oracle,
// evaluated on canned field values; the part before the comment above reset() adds the event
// observers, run against a fake field and document. Passing shows that the oracle accepts each
// scenario's exact checkpoints and rejects damaged ones; it is not evidence about any input path.

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

// One astral character in the prefix, so a code-point offset is one less than the UTF-16 one
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

  // Equal lengths and a valid caret cannot hide damage elsewhere in the field; neither can a
  // caret one unit off, missing checkpoints, an open or unbalanced composition, an untrusted
  // event or a final value reached without the checkpoints
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
  // The report carries flags, positions and counts, never the field's text (S8)
  const report = JSON.stringify(result);
  for (const checkpoint of scenario.checkpoints)
    assert.equal(report.includes(checkpoint.trim()), false, 'report contains field text');
}

const observerBoundary = script.indexOf('// The only programmatic edit or caret placement;');
assert.ok(observerBoundary > boundary, 'observer boundary missing');
const setupBoundary = script.indexOf('\nfor (const s of SCENARIOS) select.add');
assert.ok(setupBoundary > observerBoundary, 'observer setup boundary missing');
for (const scenario of SCENARIOS) {
  const listeners = {};
  const refreshListeners = {};
  let reportWrites = 0;
  const reportNode = () => new Proxy({}, {
    set(target, property, value) {
      reportWrites++;
      target[property] = value;
      return true;
    },
  });
  const field = {
    value: PREFIX + SUFFIX, selectionStart: PREFIX.length, selectionEnd: PREFIX.length,
    addEventListener(type, listener) { listeners[type] = listener; },
  };
  const nodes = {field, scenario: {}, summary: reportNode(), report: reportNode(),
    refresh: {addEventListener(type, listener) { refreshListeners[type] = listener; }}};
  const document = {
    getElementById(id) { return nodes[id]; },
    addEventListener(type, listener) { listeners[type] = listener; },
  };
  const observed = vm.runInNewContext(script.slice(0, setupBoundary) +
    '\n({setScenario(id) {scenario = SCENARIOS.find(s => s.id === id); run = newRun();}, ' +
    'result() {return evaluate(scenario, run, field.value, field.selectionStart, field.selectionEnd);}, reset})',
    {document});
  observed.setScenario(scenario.id);
  const event = (type, inputType) => {
    const before = reportWrites;
    listeners[type]({type, inputType, isTrusted: true});
    assert.equal(reportWrites, before, type + ' observer must keep reporting in memory');
  };
  const report = () => observed.result();
  event('selectionchange');
  assert.equal(report().checkpoints_reached, '0/' + scenario.checkpoints.length);
  assert.equal(report().counts.checkpoint_advances_selectionchange, 0,
               'reset selectionchange must not advance');
  let prevented = false;
  refreshListeners.pointerdown({preventDefault() { prevented = true; }});
  assert.equal(prevented, true, 'pointer refresh must cancel default focus transfer');
  event('compositionstart');
  const beforeRefusedActions = reportWrites;
  refreshListeners.click();
  observed.reset();
  assert.equal(reportWrites, beforeRefusedActions,
               'refresh and reset must leave the published report untouched while composing');
  let reached = 0;
  for (const checkpoint of scenario.checkpoints) {
    const caret = PREFIX.length + checkpoint.length;
    const committedLength = scenario.id === 'auto' && reached > 0 ?
      scenario.checkpoints[reached - 1].length :
      scenario.id === 'auto-ru' && reached > 1 ? scenario.checkpoints[reached - 1].length : 0;
    field.value = PREFIX + checkpoint + SUFFIX;
    field.selectionStart = PREFIX.length + committedLength;
    field.selectionEnd = caret;
    event('compositionupdate');
    event('input', 'insertCompositionText');
    assert.equal(report().checkpoints_reached, reached + '/' + scenario.checkpoints.length);
    field.selectionStart = caret;
    event('selectionchange');
    reached++;
    assert.equal(report().checkpoints_reached, reached + '/' + scenario.checkpoints.length);
  }
  event('compositionend');
  assert.equal(report().pass, true, 'late caret must complete exact checkpoints');
  refreshListeners.click();
  assert.equal(JSON.parse(nodes.report.textContent).pass, true,
               'explicit refresh must publish the completed run');
  assert.equal(report().counts.checkpoint_advances_selectionchange, reached);
  assert.equal(report().counts.checkpoint_advances_input, 0);
  assert.equal(report().counts.checkpoint_advances_composition, 0);

  field.selectionStart--;
  event('selectionchange');
  assert.equal(report().checks.caret, false);
  assert.equal(report().pass, false, 'later selection must not leave a stale pass');

  // A browser that collapses the caret at input time advances there, and the selectionchange
  // that follows must not count the same checkpoint again
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

for (const base of SCENARIOS) {
  const scenario = {...base, prefix: PREFIX, suffix: ' Fixture end.',
    checkpoints: base.checkpoints.map(value => value.replace(/\n$/, ''))
      .filter((value, index, values) => index === 0 || value !== values[index - 1])};
  const run = newRun();
  run.counts.compositionstart = 1;
  run.counts.compositionend = 1;
  for (const checkpoint of scenario.checkpoints) {
    const text = scenario.prefix + checkpoint + scenario.suffix;
    const caret = scenario.prefix.length + checkpoint.length;
    run.reached = advance(scenario, run.reached, text, caret, caret);
  }
  const final = scenario.checkpoints.at(-1);
  const text = scenario.prefix + final + scenario.suffix;
  const caret = scenario.prefix.length + final.length;
  assert.equal(evaluate(scenario, run, text, caret, caret).pass, true);
  assert.equal(evaluate(scenario, run, text + 'x', caret, caret).pass, false);
  assert.equal(evaluate(scenario, run, text, caret - 1, caret - 1).pass, false);
}

const windowsObservers = script.slice(script.indexOf('  function snapshot(node) {'),
  script.indexOf("  document.getElementById('reset').addEventListener('click', resetWindows, true)"));
assert.ok(windowsObservers.includes('function capture(event)'), 'Windows observers missing');
for (const control of ['input', 'rich']) {
  for (const base of SCENARIOS) {
    const caseScenario = {...base, prefix: PREFIX, suffix: ' Fixture end.',
      checkpoints: base.checkpoints.map(value => value.replace(/\n$/, ''))
        .filter((value, index, values) => index === 0 || value !== values[index - 1])};
    const listeners = {};
    let reportWrites = 0;
    const reportNode = () => new Proxy({}, {
      set(target, property, value) {
        reportWrites++;
        target[property] = value;
        return true;
      },
    });
    const nodes = {summary: reportNode(), report: reportNode()};
    const fixtureField = {
      addEventListener(type, listener) { listeners[type] = listener; },
      contains(node) { return node === fixtureField; },
      focus() { assert.fail('result publication must preserve focus without refocusing'); },
    };
    if (control === 'input') fixtureField.value = PREFIX + caseScenario.suffix;
    else fixtureField.textContent = PREFIX + caseScenario.suffix;
    let start = PREFIX.length;
    let end = start;
    const document = {
      activeElement: fixtureField,
      getElementById(id) { return nodes[id]; },
      addEventListener(type, listener) { listeners[type] = listener; },
      createRange() {
        let through = 0;
        return {
          selectNodeContents() {},
          setEnd(node, offset) { through = offset; },
          toString() { return fixtureField.textContent.slice(0, through); },
        };
      },
    };
    const window = {getSelection() { return {
      rangeCount: 1, anchorNode: fixtureField, focusNode: fixtureField,
      getRangeAt() { return {startContainer: fixtureField, startOffset: start,
        endContainer: fixtureField, endOffset: end}; },
    }; }};
    let refreshListener;
    const refresh = {addEventListener(type, listener, capture) {
      assert.equal(type, 'click');
      assert.equal(capture, true, 'Windows refresh must override the common renderer');
      refreshListener = listener;
    }};
    const observed = vm.runInNewContext(script.slice(0, boundary) +
      '\nconst selected = fixtureField; const selectedName = control; ' +
      'const controls = new Map([[control, fixtureField]]); ' +
      'let active = caseScenario; let windowsRun = newRun(); ' +
      'const chooser = {}; const select = {};\n' + windowsObservers +
      '\n({state() {return windowsRun;}, resetWindows})',
      {document, window, refresh, fixtureField, caseScenario, control});
    const event = (type, inputType) => {
      const before = reportWrites;
      listeners[type]({type, inputType, isTrusted: true, target: fixtureField});
      assert.equal(reportWrites, before, 'Windows ' + type + ' must keep reporting in memory');
    };
    const refreshReport = () => {
      let prevented = false;
      let stopped = false;
      refreshListener({preventDefault() { prevented = true; },
        stopImmediatePropagation() { stopped = true; }});
      assert.equal(prevented && stopped, true);
      assert.equal(document.activeElement, fixtureField);
    };
    event('compositionstart');
    refreshReport();
    observed.resetWindows();
    assert.equal(reportWrites, 0, 'composing refresh/reset must retain the last published report');
    for (const checkpoint of caseScenario.checkpoints) {
      event('compositionupdate');
      const text = PREFIX + checkpoint + caseScenario.suffix;
      if (control === 'input') fixtureField.value = text;
      else fixtureField.textContent = text;
      start = end = PREFIX.length + checkpoint.length;
      fixtureField.selectionStart = start;
      fixtureField.selectionEnd = end;
      event('input', 'insertCompositionText');
      event('selectionchange');
    }
    event('compositionend');
    assert.equal(observed.state().reached, caseScenario.checkpoints.length);
    assert.equal(reportWrites, 0);
    refreshReport();
    const result = JSON.parse(nodes.report.textContent);
    assert.equal(result.pass, true);
    assert.equal(result.fixture, 'keyrill-windows-public-fixture-1');
    assert.equal(result.control, control);
    assert.equal(result.counts.checkpoint_advances_input, caseScenario.checkpoints.length);
    assert.equal(result.counts.checkpoint_advances_selectionchange, 0);
    start--;
    fixtureField.selectionStart = start;
    event('selectionchange');
    refreshReport();
    assert.equal(JSON.parse(nodes.report.textContent).checks.caret, false);
  }
}
console.log('browser fixture publication: common and Windows input/rich observers keep reports ' +
            'in memory; explicit refresh and composing refusal passed');
