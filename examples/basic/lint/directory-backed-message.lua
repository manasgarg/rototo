function lint_value(value)
  if value.value == "" then
    return {
      {
        message = "value " .. value.name .. " must not be empty",
        help = "Set a non-empty message."
      }
    }
  end
  return {}
end
