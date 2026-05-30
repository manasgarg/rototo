function lint(variable)
  return {
    {
      rule = "payments/undeclared-rule",
      message = "custom lint rejected " .. variable.id
    }
  }
end
