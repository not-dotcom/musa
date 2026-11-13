'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { ok, fail, isCommand } = require('./protocol.js');

test('respuesta correcta con versión', () => {
  assert.deepEqual(ok(7, { pong: true }), {
    v: 1,
    id: 7,
    ok: true,
    data: { pong: true },
  });
});

test('respuesta de error', () => {
  const response = fail(3, 'sidecar_error', 'sin ventana');
  assert.equal(response.ok, false);
  assert.equal(response.error.code, 'sidecar_error');
});

test('solo acepta comandos documentados', () => {
  assert.equal(isCommand('play'), true);
  assert.equal(isCommand('state'), true);
  assert.equal(isCommand('rm -rf'), false);
});
