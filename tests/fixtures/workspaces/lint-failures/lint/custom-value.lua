function lint_value(value)
  return {
    {
      rule = "fixture/custom-value-rejected",
      message = "custom value lint rejected " .. value.variable.id .. "." .. value.name
    }
  }
end
