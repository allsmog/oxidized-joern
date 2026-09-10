struct Item { int len; int code; };
#define Len len + 1
int read(struct Item *p) { return p->Len; }
