function lint(variable)
  return {
    {
      rule = "rototo/not-allowed",
      message = "custom lint rejected " .. variable.id
    }
  }
end
