import { createApp } from "vue";
import { createPinia } from "pinia";
import "./assets/main.css";
import App from "./App.vue";
import CameraWindow from "./views/CameraWindow.vue";

const standaloneCamera = new URLSearchParams(window.location.search).get('cameraPlayer') === '1';
if (standaloneCamera) document.body.classList.add('camera-window-body');
createApp(standaloneCamera ? CameraWindow : App).use(createPinia()).mount("#app");
