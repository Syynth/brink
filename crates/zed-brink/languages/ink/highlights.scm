; Structural headers
(knot name: (identifier) @function.definition)
(stitch name: (identifier) @function.definition)

; Divert targets
(divert_target (identifier) @function)

; Divert / tunnel keywords
(divert "->" @keyword)
(tunnel_call "->" @keyword)
(tunnel_return) @keyword

; Choice and gather markers
(choice_mark) @keyword
(gather_mark) @keyword

; Declaration keywords
(var_decl "VAR" @keyword)
(const_decl "CONST" @keyword)
(list_decl "LIST" @keyword)
(include "INCLUDE" @keyword)
(external "EXTERNAL" @keyword)

; Declaration names
(var_decl name: (identifier) @variable)
(const_decl name: (identifier) @variable)
(list_decl name: (identifier) @variable)
(external name: (identifier) @function)
(parameter (identifier) @variable.parameter)

; Logic prefix
(logic_line "~" @operator)

; Built-in constants
(done) @constant.builtin
(end) @constant.builtin
(boolean) @constant.builtin

; Literals
(string) @string
(number) @number

; Comments
(comment_line) @comment
(block_comment) @comment

; Tags
(tag) @attribute

; Function keyword
(knot "function" @keyword)

; Knot header delimiters
(knot "==" @punctuation.delimiter)
