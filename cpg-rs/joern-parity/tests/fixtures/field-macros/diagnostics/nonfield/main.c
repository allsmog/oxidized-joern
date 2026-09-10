struct Pair { int len; int code; };
struct Item { struct Pair dl; struct Pair fc; };
#define Len dl.len
#define READ(p) ((p)->dl.len)
int read(struct Item *p) { return READ(p); }
