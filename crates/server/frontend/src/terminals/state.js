export const terminalsState = {
  /** @type {any[]} */
  terminalsData: [],
  currentFilterTab: "all",
  /** @type {any[]} */
  runningCallsData: [],
  /** @type {ReturnType<typeof setInterval> | null} */
  activeCmdCancelPoll: null,
};
