const { invoke } = window.__TAURI__.core;

const statusEl = () => document.querySelector("#status");
const recordBtn = () => document.querySelector("#record-btn");
const playBtn = () => document.querySelector("#play-btn");
const stopBtn = () => document.querySelector("#stop-btn");
const repeatCheckbox = () => document.querySelector("#repeat-checkbox");
const slotButtons = () => Array.from(document.querySelectorAll(".slot-btn"));

let isRecording = false;
let isPlaying = false;


async function selectSlot(slot) {
  await invoke("set_selected_slot", { slot });
  await refreshStatus();
}

async function refreshStatus() {
  const st = await invoke("get_status");

  statusEl().textContent = st.status;
  isRecording = st.is_recording;
  isPlaying = st.is_playing;

  recordBtn().textContent = isRecording ? "Stop Recording" : "Record (10s)";
  recordBtn().disabled = isPlaying;

  playBtn().disabled = !st.has_recording || isRecording || isPlaying;
  stopBtn().disabled = !isPlaying;

  repeatCheckbox().checked = st.repeat_playback;
  repeatCheckbox().disabled = isPlaying;

  slotButtons().forEach((btn, index) => {
    btn.classList.toggle("active", index === st.selected_slot);
    btn.classList.toggle("filled", st.slots[index]);
  });
}

async function toggleRecord() {
  if (!isRecording) {
    await invoke("start_recording", {
      controllerIndex: 0,
      fps: 60,
      maxSeconds: 10,
    });
  } else {
    await invoke("stop_recording");
  }
  await refreshStatus();
}

async function playback() {
  await invoke("set_repeat_playback", {
    enabled: repeatCheckbox().checked,
  });
  await invoke("playback");
  await refreshStatus();
}

async function stopPlayback() {
  await invoke("stop_playback");
  await refreshStatus();
}

async function onRepeatChanged() {
  await invoke("set_repeat_playback", {
    enabled: repeatCheckbox().checked,
  });
  await refreshStatus();
}

window.addEventListener("DOMContentLoaded", async () => {
  recordBtn().addEventListener("click", toggleRecord);
  playBtn().addEventListener("click", playback);
  stopBtn().addEventListener("click", stopPlayback);
  repeatCheckbox().addEventListener("change", onRepeatChanged);

  await refreshStatus();

  slotButtons().forEach((btn) => {
    btn.addEventListener("click", async () => {
      await selectSlot(Number(btn.dataset.slot));
    });
  });

  // keep UI state fresh while playback/recording runs
  setInterval(refreshStatus, 250);
});