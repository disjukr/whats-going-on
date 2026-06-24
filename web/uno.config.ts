import { defineConfig, presetUno } from "unocss";

const preflightStyles = String.raw`
*,
::before,
::after {
  box-sizing: border-box;
  border-color: #d8dde7;
  border-style: solid;
  border-width: 0;
}

html,
body,
#root {
  height: 100%;
  margin: 0;
  overflow: hidden;
}

html {
  font-size: 12px;
}

body {
  background: #f3f5f8;
  color: #20242d;
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
    "Segoe UI", sans-serif;
}

input {
  font: inherit;
  min-width: 0;
  min-height: 34px;
  border: 1px solid #c7ceda;
  border-radius: 6px;
  background: #ffffff;
  color: #20242d;
  padding: 0 10px;
}

input:focus {
  outline: 2px solid #4f8cff;
  outline-offset: 1px;
}

.machine-title-button.checking .machine-title-text {
  background: linear-gradient(
    100deg,
    #20242d 0%,
    #20242d 34%,
    #ffffff 50%,
    #20242d 66%,
    #20242d 100%
  );
  background-clip: text;
  background-size: 240% 100%;
  color: transparent;
  animation: machine-title-shimmer 1.4s linear infinite;
}

@keyframes machine-title-shimmer {
  from {
    background-position: 120% 0;
  }

  to {
    background-position: -120% 0;
  }
}
`;

export default defineConfig({
  presets: [presetUno()],
  preflights: [
    {
      getCSS: () => preflightStyles,
    },
  ],
});
