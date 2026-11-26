const PATHS = {
  minimize: '<path d="M5 12h14" />',
  pin: '<path d="m9 3 6 6M12 3l9 9-4 1-5 5-1 4-9-9 4-1 5-5 1-4Z" /><path d="m8 16-5 5" />',
  newChat: '<path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z" />',
  plus: '<path d="M12 5v14M5 12h14" />',
  settings: '<path d="M4 7h10M18 7h2M4 17h2M10 17h10" /><circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" />',
  history: '<circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" />',
  back: '<path d="m15 18-6-6 6-6" />',
};

export function icon(name) {
  const path = PATHS[name];
  if (!path) return "";
  return `
  <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    ${path}
  </svg>
`;
}
