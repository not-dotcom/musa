'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { unresolvedIds, queueAfterRemoving, setQueueResilient } = require('./queue.js');

test('extrae los ids no resueltos del error de MusicKit', () => {
  const error = new Error(
    'Error: [mk-007] NOT_FOUND; One or more items could not be resolved: 1550626764, 1550626766',
  );
  assert.deepEqual(
    unresolvedIds(error, ['1513276337', '1550626764', '1550626766']),
    ['1550626764', '1550626766'],
  );
});

test('también entiende otros formatos de lista', () => {
  const error = new Error('NOT_FOUND; items not found: [123, 456]');
  assert.deepEqual(unresolvedIds(error, ['123', '456', '789']), ['123', '456']);
});

test('sin ids reconocibles no inventa omisiones', () => {
  assert.deepEqual(unresolvedIds(new Error('timeout del sidecar'), ['123']), []);
});

test('conserva la canción de inicio y su posición', () => {
  const result = queueAfterRemoving(['a', 'b', 'c', 'd'], 2, ['a']);
  assert.deepEqual(result, { ids: ['b', 'c', 'd'], index: 1 });
});

test('si desaparece la canción de inicio empieza en la siguiente', () => {
  const result = queueAfterRemoving(['a', 'b', 'c', 'd'], 2, ['c']);
  assert.deepEqual(result, { ids: ['a', 'b', 'd'], index: 2 });
});

test('si desaparecen todas no queda índice fuera de rango', () => {
  const result = queueAfterRemoving(['a', 'b'], 1, ['a', 'b']);
  assert.deepEqual(result, { ids: [], index: 0 });
});

test('reintenta la cola sin las canciones que Apple ya no resuelve', async () => {
  const calls = [];
  const setQueue = async (args) => {
    calls.push(args);
    if (calls.length === 1) {
      throw new Error(
        'Error: [mk-007] NOT_FOUND; One or more items could not be resolved: 1550626764, 1550626766',
      );
    }
  };
  const ids = ['1513276337', '1550626764', '1550626766', '1463409605'];
  const result = await setQueueResilient(setQueue, { song_ids: ids, index: 1, play: true });

  assert.equal(calls.length, 2);
  assert.deepEqual(calls[1], {
    song_ids: ['1513276337', '1463409605'],
    index: 1,
    play: true,
  });
  assert.deepEqual(result.queue, ['1513276337', '1463409605']);
  assert.equal(result.index, 1);
  assert.deepEqual(result.dropped, ['1550626764', '1550626766']);
});

test('falla si ninguna canción de la lista está disponible', async () => {
  const setQueue = async () => {
    throw new Error('NOT_FOUND; items could not be resolved: 123');
  };
  await assert.rejects(
    setQueueResilient(setQueue, { song_ids: ['123'], index: 0, play: true }),
    /ninguna canción de la lista está disponible/,
  );
});

test('no oculta errores ajenos al catálogo', async () => {
  const setQueue = async () => {
    throw new Error('MusicKit no está listo');
  };
  await assert.rejects(
    setQueueResilient(setQueue, { song_ids: ['123'], index: 0, play: true }),
    /MusicKit no está listo/,
  );
});
