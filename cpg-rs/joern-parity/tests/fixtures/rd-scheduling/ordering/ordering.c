int input(void);
void observe(int value);
#define VALUE(value) ((value) + 1)
void empty_true(int condition) { if (condition) {} else input(); }
void empty_false(int condition) { if (condition) input(); else {} }
void both_empty(int condition) { if (condition) {} else {} }
void switch_exit(int value) { switch (value) { case 1: break; default: input(); } }
void switch_no_default(int value) { switch (value) { case 1: input(); break; case 2: observe(value); } }
void nested_for(int first, int second) { input(); for (; first; first--) { for (; second; second--) observe(first); } }
void nested_while(int first, int second) { input(); while (first) { first--; while (second) second--; } }
void nested_do(int first, int second) { input(); do { do { second--; } while (second); first--; } while (first); }
int return_inside_block(int value) { { observe(value); return value; } }
int branch_macro(int condition, int value) { return condition ? VALUE(value) : input(); }
int short_circuit(int condition, int value) { return (condition && input()) || VALUE(value); }
int multiple_returns(int value) { input(); while (value) { if (value == 2) return input(); if (value == 3) return 1; value--; } return input(); }
