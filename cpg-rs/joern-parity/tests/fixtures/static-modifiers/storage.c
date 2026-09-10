int static after_type(int value) { return value; }
const static int after_const(int value) { return value; }
static const int before_const(int value) { return value; }
static
int static_newline(int value) { return value; }
static /* comment */ int static_comment(int value) { return value; }
/* leading */ static int leading_comment(int value) { return value; }
int static after_type_prototype(int value);
inline static int after_inline_prototype(int value);
typedef int static_type;
static_type prefixed_type(int value) { return value; }
typedef int static$type;
static$type dollar_type(int value) { return value; }
