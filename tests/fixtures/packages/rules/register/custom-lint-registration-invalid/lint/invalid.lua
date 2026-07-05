function register(lint)
  lint:rule({
    id = "payments/check",
    title = "Payments check",
    help = "Fix the payments policy.",
    target = "enum=tier",
    handler = "check",
  })
end

function check(package, target)
  return {}
end
