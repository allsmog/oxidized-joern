long signed int identity(long signed int value) { long signed int local = value; return local; }
long signed int declared(long signed int value);
long signed int call_definition(long signed int value) { return identity(value); }
long signed int call_prototype(long signed int value) { return declared(value); }
