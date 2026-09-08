int control_tick(int x) { return x; }
int scalar_if(int x) { if (x) return 1; return 0; }
int pointer_if(int *p) { if (p) return *p; return 0; }
int explicit_if(int x) { if (x != 0) return 1; return 0; }
int negation_if(int x) { if (!x) return 1; return 0; }
int pointer_negation_if(int *p) { if (!p) return 1; return 0; }
int scalar_while(int x) { while (x) x = x - 1; return x; }
int pointer_while(int *p) { while (p) p = 0; return 0; }
int scalar_do(int x) { do x = x - 1; while (x); return x; }
int pointer_do(int *p) { do p = 0; while (p); return 0; }
int scalar_for(int x) { for (; x; x = x - 1) control_tick(x); return x; }
int explicit_loops(int x) { while (x > 2) x--; do x--; while (x > 1); for (; x > 0; x--) control_tick(x); return x; }
int empty_for(int x) { for (;;) break; return x; }
int arithmetic_if(int x) { if (x + 1) return x; return 0; }
int call_if(int x) { if (control_tick(x)) return x; return 0; }
int literal_if(int x) { if (1) return x; return 0; }
int dereference_if(int *p) { if (*p) return *p; return 0; }
int and_if(int x, int y) { if (x && y) return x; return y; }
int paren_if(int x) { if (((x))) return x; return 0; }
int boolean_if(_Bool x) { if (x) return 1; return 0; }
int array_if(int p[2]) { if (p) return p[0]; return 0; }
int assignment_if(int x) { if (x = control_tick(x)) return x; return 0; }
int no_update_for(int x) { for (;x;) x--; return x; }
int no_condition_for(int x) { for (;;x--) if (x == 0) break; return x; }
int init_only_for(int x) { for (x = 2;;) if (--x == 0) break; return x; }
int declared_for(int x) { for (int i = x; i; i--) control_tick(i); return x; }
int declared_empty_for(int x) { for (int i;;) break; return x; }
int empty_while(int x) { while (x); return x; }
int empty_do(int x) { do; while (x); return x; }
int nested_while(int x) { while (x) if (--x > 2) continue; else break; return x; }
int nested_for(int x) { for (;x;x--) if (x > 2) continue; else break; return x; }
int return_while(int x) { while (x) return x; return 0; }
int return_do(int x) { do return x; while (x); return 0; }
int return_for(int x) { for (;x;) return x; return 0; }
int pointer_for(int *p) { for (;p;) p = 0; return 0; }
int multi_init_for(int x) { for (int i=x, j=x; i; i--) control_tick(j); return x; }
int multi_empty_for(int x) { for (int i,j;;) break; return x; }
int comma_loop(int x) { int y=x; for (x=2,y=3; x; x--,y--) control_tick(x),control_tick(y); return y; }
int empty_all_for(int x) { for (;;); return x; }
int pointer_compare(int *p) { if (p != 0) return 1; if (!p) return 0; return 2; }
int nested_do(int x) { do if (--x > 2) continue; else break; while(x); return x; }
