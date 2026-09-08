extern int (paren_defined)(int value);
int (paren_defined)(int value) { return value; }
int parenthesized_definition_caller(int value) { return paren_defined(value); }
