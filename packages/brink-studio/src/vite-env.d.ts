/// <reference types="vite/client" />

declare module "*.ink.txt?raw" {
  const content: string;
  export default content;
}
