export function getNormalizedCoords(canvas, event, scaleMode = "fit") {
  const rect = canvas.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return { x: 0, y: 0 };

  if (scaleMode === "stretch") {
    const mouseX = Math.max(0, Math.min(rect.width, event.clientX - rect.left));
    const mouseY = Math.max(
      0,
      Math.min(rect.height, event.clientY - rect.top),
    );
    return {
      x: Math.round((mouseX / rect.width) * 65535),
      y: Math.round((mouseY / rect.height) * 65535),
    };
  }

  if (scaleMode === "original") {
    const offsetX = Math.max(0, (rect.width - canvas.width) / 2);
    const offsetY = Math.max(0, (rect.height - canvas.height) / 2);
    const mouseX = Math.max(
      0,
      Math.min(canvas.width, event.clientX - rect.left - offsetX),
    );
    const mouseY = Math.max(
      0,
      Math.min(canvas.height, event.clientY - rect.top - offsetY),
    );
    return {
      x: Math.round((mouseX / canvas.width) * 65535),
      y: Math.round((mouseY / canvas.height) * 65535),
    };
  }

  const frameAspect = canvas.width / canvas.height;
  const rectAspect = rect.width / rect.height;

  let displayW;
  let displayH;
  let offsetX;
  let offsetY;
  if (rectAspect > frameAspect) {
    displayH = rect.height;
    displayW = displayH * frameAspect;
    offsetX = (rect.width - displayW) / 2;
    offsetY = 0;
  } else {
    displayW = rect.width;
    displayH = displayW / frameAspect;
    offsetX = 0;
    offsetY = (rect.height - displayH) / 2;
  }

  const mouseX = event.clientX - rect.left - offsetX;
  const mouseY = event.clientY - rect.top - offsetY;
  const clampedX = Math.max(0, Math.min(displayW, mouseX));
  const clampedY = Math.max(0, Math.min(displayH, mouseY));
  return {
    x: Math.round((clampedX / displayW) * 65535),
    y: Math.round((clampedY / displayH) * 65535),
  };
}

export function pointerToDesktopEvent({ coords, monitors, displayIndex }) {
  if (monitors && monitors.length > 0) {
    const monitor = monitors.find((item) => item.display_index === displayIndex);
    if (monitor) {
      const px = monitor.x + Math.round((coords.x / 65535) * monitor.width);
      const py = monitor.y + Math.round((coords.y / 65535) * monitor.height);
      return {
        action: "MouseMovePixel",
        data: { x: Math.max(0, px), y: Math.max(0, py) },
      };
    }
  }
  return { action: "MouseMove", data: coords };
}
