signed short identity(signed short value) { signed short local = value; return local; }
signed short declared(signed short value);
signed short call_definition(signed short value) { return identity(value); }
signed short call_prototype(signed short value) { return declared(value); }
