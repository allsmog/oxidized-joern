struct Pair { int len; int code; };
struct Item { struct Pair dl; struct Pair fc; };
#define FIELD dl.len
#define Len FIELD
#define READ(p) ((p)->Len)
int read(struct Item *p) { return READ(p); }
