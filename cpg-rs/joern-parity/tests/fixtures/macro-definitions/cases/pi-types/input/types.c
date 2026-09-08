#define ID(x) x
#define IDP(x) (x)
#define PI (ID(3.14))
#define DIRECT (3.14)
#define DIRECT_RAW 3.14
#define A B
#define B 17
#define AP (B)
#define NEST (IDP(3.14))
#define PI_NOP ID(3.14)
#define SECOND ID
#define PI_SECOND (SECOND(3.14))
#define OBJECT_A OBJECT_B
#define OBJECT_B DIRECT
#define FN() 3.14
#define FNOBJ FN()
#define OP (1 + 2)
#define CAST ((double)17)
double pi(void) { return PI; }
double direct(void) { return DIRECT; }
double direct_raw(void) { return DIRECT_RAW; }
int object_chain(void) { return A; }
int object_paren(void) { return AP; }
double nested_paren(void) { return NEST; }
double nested_raw(void) { return PI_NOP; }
double function_alias(void) { return PI_SECOND; }
double nested_objects(void) { return OBJECT_A; }
double zero_arg(void) { return FNOBJ; }
int operation(void) { return OP; }
double casted(void) { return CAST; }
double initializer(void) { double x = PI; return x; }
void expression_statement(void) { PI; }
double direct_id(void) { return ID(3.14); }
double direct_idp(void) { return IDP(3.14); }
