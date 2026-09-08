struct Item { int Len; };
#define Len(x) (x)
int read(struct Item *p) { return p->Len; }
