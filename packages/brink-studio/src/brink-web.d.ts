declare module "brink-web" {
  export class StoryRunner {
    constructor(story_bytes: Uint8Array);
    choose(index: number): void;
    continue_story(): string;
    reset(): void;
    free(): void;
    [Symbol.dispose](): void;
  }

  export function compile(source: string): string;
  export function semantic_tokens(source: string): string;
  export function token_modifier_names(): string;
  export function token_type_names(): string;

  // IDE features
  export function completions(source: string, offset: number): string;
  export function hover(source: string, offset: number): string;
  export function goto_definition(source: string, offset: number): string;
  export function find_references(source: string, offset: number): string;
  export function prepare_rename(source: string, offset: number): string;
  export function rename(source: string, offset: number, new_name: string): string;
  export function code_actions(source: string, offset: number): string;
  export function inlay_hints(source: string, start: number, end: number): string;
  export function signature_help(source: string, offset: number): string;
  export function folding_ranges(source: string): string;
  export function document_symbols(source: string): string;
  export function format_document(source: string): string;

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function init(
    module_or_path?: InitInput | Promise<InitInput> | { module_or_path: InitInput | Promise<InitInput> },
  ): Promise<unknown>;
}
