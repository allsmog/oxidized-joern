int source(void);
int other(void);
int identity(int value);
void sink(int value);
#define VALUE_MACRO(x) ((x) + 1)
#define SINK_MACRO(x) sink(x)
int rd_ternary_call_literal(int c) { return c ? source() : 1; }
int rd_ternary_literal_call(int c) { return c ? 1 : source(); }
int rd_ternary_call_identifier(int c, int x) { return c ? source() : x; }
int rd_ternary_identifier_call(int c, int x) { return c ? x : source(); }
int rd_ternary_nested(int c, int x) { return c ? (x ? 1 : other()) : source(); }
int rd_ternary_macro(int c, int x) { return c ? VALUE_MACRO(x) : other(); }
int rd_ternary_assignment(int c, int x) { return c ? (x = source()) : (x = other()); }
void rd_if_call_literal(int c) { if (c) source(); else 1; }
void rd_if_literal_call(int c) { if (c) 1; else source(); }
void rd_if_identifier_call(int c, int x) { if (c) x; else source(); }
void rd_if_call_identifier(int c, int x) { if (c) source(); else x; }
void rd_if_empty_call(int c) { if (c) {} else source(); }
void rd_if_call_empty(int c) { if (c) source(); else {} }
int rd_if_returns(int c) { if (c) return source(); else return 1; }
int rd_if_returns_reverse(int c) { if (c) return 1; else return source(); }
int rd_short_and_call(int c) { return c && source(); }
int rd_short_and_literal(int c) { return source() && c; }
int rd_short_or_macro(int c) { return c || VALUE_MACRO(c); }
int rd_short_nested(int c, int x) { return (c && source()) || (x && other()); }
void rd_nested_while(int x, int y) { source(); while (x) { while(y) { y--; } x--; } }
void rd_nested_while_tail(int x, int y) { source(); while (x) { x--; while(y) { y--; } } }
void rd_nested_do(int x, int y) { source(); do { do {y--;} while(y); x--; } while(x); }
void rd_nested_for(int x, int y) { source(); for(;x;x--) { for(;y;y--) { sink(x); } } }
void rd_nested_switch_loop(int x, int y) { source(); while(x) { switch(y) { case 1: break; default: sink(y); } x--; } }
void rd_switch_returns(int x) { switch(x) { case 1: return; default: source(); } }
void rd_switch_breaks(int x) { switch(x) { case 1: break; default: source(); } }
void rd_switch_default_first(int x) { switch(x) { default: source(); break; case 1: other(); } }
void rd_switch_default_middle(int x) { switch(x) { case 1: source(); break; default: other(); break; case 2: sink(x); } }
int rd_loop_multiple_returns(int x) { source(); while(x) { if(x==2) return source(); if(x==3) return 1; x--; } return other(); }
void rd_loop_macro(int x) { source(); while(x) { SINK_MACRO(x); x--; } }
void rd_branch_macro(int x) { if(x) SINK_MACRO(x); else source(); }
void rd_empty_block_branch(int x) { source(); if(x) { {} } else { { sink(x); } } }
