declare module "brink-web" {
  export class StoryRunner {
    constructor(story_bytes: Uint8Array);
    choose(index: number): void;
    continue_story(): string;
    reset(): void;
    free(): void;
    [Symbol.dispose](): void;
  }

  export class EditorSession {
    constructor();
    update_source(source: string): void;
    update_file(path: string, source: string): void;
    remove_file(path: string): void;
    set_active_file(path: string): boolean;
    active_file(): string;
    list_files(): string;
    get_file_source(path: string): string;
    file_symbols(path: string): string;
    compile_project(entry: string): string;
    project_outline(): string;
    line_contexts(): string;
    semantic_tokens(): string;
    completions(offset: number): string;
    hover(offset: number): string;
    goto_definition(offset: number): string;
    find_references(offset: number): string;
    prepare_rename(offset: number): string;
    rename(offset: number, new_name: string): string;
    code_actions(offset: number): string;
    inlay_hints(start: number, end: number): string;
    signature_help(offset: number): string;
    folding_ranges(): string;
    document_symbols(): string;
    format_document(): string;
    convert_element(offset: number, target: string): string;
    free(): void;
    [Symbol.dispose](): void;
  }

  export function compile(source: string): string;
  export function token_type_names(): string;
  export function token_modifier_names(): string;

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function init(
    module_or_path?: InitInput | Promise<InitInput> | { module_or_path: InitInput | Promise<InitInput> },
  ): Promise<unknown>;
}
