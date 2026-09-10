signed long int identity(signed long int value) { signed long int local = value; return local; }
signed long int declared(signed long int value);
signed long int call_definition(signed long int value) { return identity(value); }
signed long int call_prototype(signed long int value) { return declared(value); }
