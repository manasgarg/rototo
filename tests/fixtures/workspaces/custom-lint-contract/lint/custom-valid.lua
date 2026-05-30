function lint(variable)
  return {
    {
      rule = "payments/max-token-budget",
      message = "custom lint rejected " .. variable.id
    }
  }
end
