import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import { I18n } from "./i18n/i18n";
import { useLanguageStore } from "./stores/languageStore";
import "./styles/app.css";

I18n.init(useLanguageStore.getState().language);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
