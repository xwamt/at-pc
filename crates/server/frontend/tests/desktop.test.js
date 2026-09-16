import assert from "node:assert/strict";
import { test } from "node:test";

import {
  getNormalizedCoords,
  pointerToDesktopEvent,
} from "../src/desktop/index.js";

function fakeCanvas({ width, height, rect }) {
  return {
    width,
    height,
    getBoundingClientRect() {
      return rect;
    },
  };
}

test("getNormalizedCoords stretch mode maps the full element box to 0..65535", () => {
  const canvas = fakeCanvas({
    width: 1920,
    height: 1080,
    rect: { left: 10, top: 20, width: 400, height: 200 },
  });
  const start = getNormalizedCoords(
    canvas,
    { clientX: 10, clientY: 20 },
    "stretch",
  );
  const end = getNormalizedCoords(
    canvas,
    { clientX: 410, clientY: 220 },
    "stretch",
  );
  assert.deepEqual(start, { x: 0, y: 0 });
  assert.deepEqual(end, { x: 65535, y: 65535 });
});

test("getNormalizedCoords original mode accounts for letterbox offsets", () => {
  const canvas = fakeCanvas({
    width: 200,
    height: 100,
    rect: { left: 0, top: 0, width: 400, height: 300 },
  });
  const coords = getNormalizedCoords(
    canvas,
    { clientX: 100, clientY: 100 },
    "original",
  );
  assert.deepEqual(coords, { x: 0, y: 0 });
});

test("getNormalizedCoords fit mode maps the contained picture, not the black bars", () => {
  const canvas = fakeCanvas({
    width: 200,
    height: 100,
    rect: { left: 0, top: 0, width: 400, height: 400 },
  });
  const leftBar = getNormalizedCoords(
    canvas,
    { clientX: 0, clientY: 200 },
    "fit",
  );
  const pictureRight = getNormalizedCoords(
    canvas,
    { clientX: 400, clientY: 200 },
    "fit",
  );
  assert.equal(leftBar.x, 0);
  assert.equal(pictureRight.x, 65535);
});

test("pointerToDesktopEvent uses pixel coordinates when the active monitor is known", () => {
  const event = pointerToDesktopEvent({
    coords: { x: 32767, y: 0 },
    monitors: [
      { display_index: 1, x: 1920, y: 0, width: 1920, height: 1080 },
    ],
    displayIndex: 1,
  });
  assert.equal(event.action, "MouseMovePixel");
  assert.equal(event.data.x, 1920 + Math.round((32767 / 65535) * 1920));
  assert.equal(event.data.y, 0);
});

test("pointerToDesktopEvent falls back to normalized MouseMove", () => {
  const event = pointerToDesktopEvent({
    coords: { x: 12, y: 34 },
    monitors: [],
    displayIndex: 0,
  });
  assert.deepEqual(event, { action: "MouseMove", data: { x: 12, y: 34 } });
});
