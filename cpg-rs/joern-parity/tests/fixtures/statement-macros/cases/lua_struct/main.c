struct Value { int value; int tag; };
#define SET(L,p,v) { struct Value *slot=(p); slot->value=(v); slot->tag=1; use(L); }
void use(int v);
void f(int L, struct Value *p, int v) { SET(L,p,v); }
