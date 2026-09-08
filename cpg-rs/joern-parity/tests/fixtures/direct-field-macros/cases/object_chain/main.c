struct Item { int len; int code; };
#define FIELD len
#define Len FIELD
int read(struct Item *p) { return p->Len; }
