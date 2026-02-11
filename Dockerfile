# Use the official Bun image as a base
FROM oven/bun:latest

# Set the working directory inside the container
WORKDIR /app

# Install Ghostscript and healthcheck tools
# Update package lists and install ghostscript + curl for Coolify healthchecks
RUN apt-get update \
  && apt-get install -y ghostscript curl poppler-utils \
  && rm -rf /var/lib/apt/lists/*

# Copy package.json and bun.lockb to leverage Docker cache
# This step ensures that bun install is only re-run if dependencies change
COPY package.json bun.lock ./

# Install Bun dependencies
RUN bun install --production

# Copy the rest of your application code
COPY . .

# Expose the port your application listens on
# Your server listens on process.env.PORT or 9001
EXPOSE 9001

# Command to run the application
# Use 'bun run start' as defined in your package.json
CMD ["bun", "run", "start"]
