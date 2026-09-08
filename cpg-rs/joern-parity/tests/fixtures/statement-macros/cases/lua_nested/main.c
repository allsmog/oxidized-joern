struct Value { int value; int tag; };
#define setobj(p,v) { struct Value *slot=(p); const struct Value *from=(v); slot->value=from->value; slot->tag=from->tag; }
#define setobjs2s(L,p,v) { setobj(p,v); use(L); }
void use(int v);
void f(int L, struct Value *p, struct Value *v) { setobjs2s(L,p,v); }
