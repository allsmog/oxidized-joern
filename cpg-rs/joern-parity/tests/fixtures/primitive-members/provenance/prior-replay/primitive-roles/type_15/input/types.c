long int identity(long int value) { long int local = value; return local; }
long int declared(long int value);
long int call_definition(long int value) { return identity(value); }
long int call_prototype(long int value) { return declared(value); }
